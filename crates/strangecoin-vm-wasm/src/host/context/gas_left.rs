//! `gas_left() -> i64` — returns the remaining gas of the current call frame.
//!
//! Deterministic (pure view of execution state), charged per ADR-0014
//! (`HOST_CALL_GAS_LEFT = 3`) before the value is read, so probing the
//! budget is never free.

use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{charge, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "gas_left", gas_left)
        .map(|_| ())
        .map_err(Into::into)
}

fn gas_left(mut caller: Caller<'_, HostState>) -> Result<i64, wasmi::Error> {
    charge(&mut caller, HostCallWeight::GasLeft)?;
    Ok(caller.get_fuel()? as i64)
}
