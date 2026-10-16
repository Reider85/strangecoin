//! Log host functions (S1.5-P10): `emit_log`. Receipt formalization is
//! S1.5-P14.

pub mod emit_log;

use wasmi::Linker;

use super::HostState;

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    emit_log::register(linker)
}
