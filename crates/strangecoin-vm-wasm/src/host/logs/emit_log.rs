//! `emit_log(topics_ptr, topics_count, data_ptr, data_len) -> i32`.
//!
//! Appends a `Log { address: current callee, topics, data }` to the frame's
//! log list (S1.5-P14 collects these into receipts). At most 4 topics, each
//! 32 bytes (Ethereum-style indexed fields). Returns `1` on success. Gas:
//! 375 + 375 per topic (ADR-0014).

use strangecoin_core::vm::Log;
use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{abi, charge, checked_u32, HostState};

/// Ethereum-style cap on indexed topics per log.
pub const MAX_TOPICS: usize = 4;

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "emit_log", emit_log)
        .map(|_| ())
        .map_err(Into::into)
}

fn emit_log(
    mut caller: Caller<'_, HostState>,
    topics_ptr: i32,
    topics_count: i32,
    data_ptr: i32,
    data_len: i32,
) -> Result<i32, wasmi::Error> {
    let topics_count_u32 = checked_u32(topics_count, "topics_count")?;
    let topics_count_usize = topics_count_u32 as usize;
    if topics_count_usize > MAX_TOPICS {
        return Err(wasmi::Error::new(format!(
            "emit_log: at most {MAX_TOPICS} topics, got {topics_count_usize}"
        )));
    }
    let weight = HostCallWeight::EmitLog {
        topics: topics_count_u32 as u64,
    };
    charge(&mut caller, weight)?;
    let topics_ptr = checked_u32(topics_ptr, "topics_ptr")?;
    let data_ptr = checked_u32(data_ptr, "data_ptr")?;
    let data_len = checked_u32(data_len, "data_len")?;

    let mut topics = Vec::with_capacity(topics_count_usize);
    for i in 0..topics_count_usize {
        let ptr = topics_ptr
            .checked_add((i * 32) as u32)
            .ok_or_else(|| wasmi::Error::new("emit_log: topics pointer overflow"))?;
        topics.push(abi::read_array32(&caller, ptr)?);
    }
    let data = abi::read_bytes(&caller, data_ptr, data_len)?;

    let state = caller.data_mut();
    if state.ctx.read_only {
        return Err(wasmi::Error::new(
            "emit_log: called from a read-only (staticcall) frame",
        ));
    }
    state.logs.push(Log {
        address: state.ctx.callee.clone(),
        topics,
        data,
    });
    Ok(1)
}
