//! Host function registration (S1.5-P09 starts the host surface; the full
//! set of 13 functions lands in S1.5-P10).
//!
//! All host functions are registered under the `sc` namespace and metered via
//! [`charge_host_call`](crate::gas::charge_host_call) before doing work
//! (ADR-0014). Wasmi's sandbox exposes no I/O — these functions are the only
//! channel a contract has to the outside world (threat V-21).

use wasmi::{Linker, Caller};

use crate::gas::{charge_host_call, HostCallWeight};

mod gas_left;

/// Register every implemented host function on `linker`.
pub fn register(linker: &mut Linker<()>) -> Result<(), wasmi::Error> {
    gas_left::register(linker)
}

/// Charge `weight` against the caller's fuel (shared entry point for all
/// host functions).
pub(crate) fn charge(caller: &mut Caller<'_, ()>, weight: HostCallWeight) -> Result<(), wasmi::Error> {
    charge_host_call(caller, weight.gas())
}
