//! Context host functions (S1.5-P10/FBP03): `block_context`, `gas_left`,
//! `input_read`. Pure deterministic views of the executing frame — no
//! wall-clock, no entropy.

pub mod block_context;
pub mod gas_left;
pub mod input_read;

use wasmi::Linker;

use super::HostState;

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    block_context::register(linker)?;
    gas_left::register(linker)?;
    input_read::register(linker)?;
    Ok(())
}
