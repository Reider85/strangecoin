//! `0x06 bls_aggregate` — BLS12-381 aggregate signature verification.
//!
//! Reserved until Stage 7 (hybrid PoS): no BLS12-381 dependency ships in
//! the MVP (ADR-0015). Input/output layout is reserved; any call reverts
//! with [`VmError::NotImplemented`](strangecoin_core::vm::VmError::NotImplemented)
//! after the declared gas (12000) is charged.

use super::{PrecompileError, PrecompileOutput};
use crate::gas::HostCallWeight;

pub fn run(_data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
    Err(PrecompileError::NotImplemented {
        feature: "bls_aggregate".to_string(),
        gas_used: HostCallWeight::BlsVerify.gas(),
    })
}
