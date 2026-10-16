//! `bls_verify` — reserved until Stage 7 (no BLS12-381 dependency yet).
//!
//! Charges the declared weight (12000, ADR-0014) and fails the frame with
//! `NotImplemented`, mirroring precompile `0x06 bls_aggregate`. The signature
//! is fixed now so contracts and tests can be written against it.

use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{charge, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "bls_verify", bls_verify)
        .map(|_| ())
        .map_err(Into::into)
}

fn bls_verify(mut caller: Caller<'_, HostState>) -> Result<i32, wasmi::Error> {
    charge(&mut caller, HostCallWeight::BlsVerify)?;
    Err(wasmi::Error::new(
        "bls_verify: not implemented until Stage 7 (BLS12-381)",
    ))
}
