//! Contract storage host functions (S1.5-P10): `storage_read`,
//! `storage_write`. Reads/writes hit the frame's storage overlay; flushing
//! the overlay into the Sparse Merkle Tree and updating
//! `ContractAccount.storage_root` is S1.5-P15.

pub mod storage_read;
pub mod storage_write;

use wasmi::Linker;

use super::HostState;

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    storage_read::register(linker)?;
    storage_write::register(linker)?;
    Ok(())
}
