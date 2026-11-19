//! `value() -> i64` — the executing frame's entry value (AI-P03).
//!
//! Returns `ctx.value`: the SC amount credited to the callee when the frame
//! started (top-level `CallTx.value`, or the `value` argument of a nested
//! `call`). Added for the `EscrowVault` deposit↔value binding (ADR-0030
//! amendment): a pooled escrow vault must record how much a deposit
//! *actually* moved — calldata alone would let an attacker deposit 1 SC
//! while claiming 1_000_000 SC and later drain co-mingled escrows through
//! `release`. Without a value view the contract cannot bind the record to
//! the credit, so the host gained this read-only snapshot.
//!
//! Deterministic (pure view of execution state — no clock, no RNG). Gas: 3
//! (ADR-0014 class: the other context snapshots). The `u64 → i64` cast
//! saturates at `i64::MAX`: SC amounts above 2^63 are unreachable given the
//! emission schedule (and the nested `call` ABI already passes value as
//! `i64`).

use crate::gas::HostCallWeight;
use crate::host::{charge, HostState};
use wasmi::{Caller, Linker};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "value", value)
        .map(|_| ())
        .map_err(Into::into)
}

fn value(mut caller: Caller<'_, HostState>) -> Result<i64, wasmi::Error> {
    charge(&mut caller, HostCallWeight::Value)?;
    Ok(caller.data().ctx.value.min(i64::MAX as u64) as i64)
}
