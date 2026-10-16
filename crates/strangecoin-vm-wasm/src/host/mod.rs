//! Host functions (S1.5-P10) — the only channel between a contract and the
//! outside world.
//!
//! All host functions are registered under the `sc` namespace and metered via
//! [`charge`](crate::host::charge) before doing work (ADR-0014). Wasmi's
//! sandbox exposes no I/O — a malicious module can only reach what is
//! registered here (threat V-21), so every function validates its input
//! before touching state and stays deterministic (no time, no random, no
//! network).
//!
//! Byte buffers cross the WASM/host boundary through the memory-pointer
//! convention documented in [`abi`]: inputs as `(ptr, len)` pairs, outputs
//! into a caller-provided out-buffer.

pub(crate) mod abi;
pub mod calls;
pub mod context;
pub mod crypto;
pub mod logs;
pub mod state;
pub mod storage;

pub use state::HostState;

use wasmi::{Caller, Linker};

use crate::gas::{charge_host_call, HostCallWeight};

/// Register every host function on `linker`.
pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    crypto::register(linker)?;
    storage::register(linker)?;
    calls::register(linker)?;
    context::register(linker)?;
    logs::register(linker)?;
    Ok(())
}

/// Charge `weight` against the caller's fuel (shared entry point for all
/// host functions).
pub(crate) fn charge(
    caller: &mut Caller<'_, HostState>,
    weight: HostCallWeight,
) -> Result<(), wasmi::Error> {
    charge_host_call(caller, weight.gas())
}

/// Convert a WASM-side `i32` length/offset into `u32`, rejecting negatives.
pub(crate) fn checked_u32(value: i32, what: &str) -> Result<u32, wasmi::Error> {
    u32::try_from(value).map_err(|_| wasmi::Error::new(format!("host ABI: negative {what}")))
}
