//! Precompiles — built-in contracts at reserved addresses `0x01..=0x07`
//! (ADR-0015, S1.5-P11).
//!
//! A precompile is a pure function `input -> output` reachable through the
//! normal call path (`call("0x0N", data)` from a contract, or a `CallTx`
//! with `to` in the reserved range) with no WASM bytecode behind it.
//! [`dispatch_precompile`] is the router; `WasmiExecutor::execute` calls it
//! before WASM instantiation when the callee is a precompile address.
//!
//! Failure semantics (ADR-0015): invalid input consumes the full gas of the
//! call frame and returns failure; precompiles never panic. Every input
//! length is validated before any allocation or crypto work (threat V-21).

pub mod blake3;
pub mod bls_aggregate;
pub mod ecrecover;
pub mod ed25519_verify;
pub mod keccak;
pub mod modexp;
pub mod sha256;

use crate::gas::HostCallWeight;

/// Successful precompile execution: output bytes and the gas charged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrecompileOutput {
    pub output: Vec<u8>,
    pub gas_used: u64,
}

/// Why a precompile rejected the call.
///
/// Both variants are consensus-visible; the executor maps them per the
/// ADR-0015 failure semantics (full burn on `InvalidInput`, declared gas
/// on `NotImplemented`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrecompileError {
    /// Malformed input (bad length, bad encoding, size overflow).
    /// Consumes the full gas of the call frame.
    InvalidInput(String),
    /// Reserved precompile not implemented yet (`0x06` until Stage 7).
    /// Consumes the precompile's declared gas, then reverts.
    NotImplemented { feature: String, gas_used: u64 },
}

impl PrecompileError {
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        PrecompileError::InvalidInput(msg.into())
    }
}

/// Gas charged for `ed25519_verify` (ADR-0015).
pub(crate) const GAS_ED25519_VERIFY: u64 = 2000;

/// Gas charged for `modexp`: 50 + 50 per byte of result (ADR-0015).
pub(crate) fn modexp_gas(result_bytes: u64) -> u64 {
    50u64.saturating_add(50u64.saturating_mul(result_bytes))
}

/// Hash-precompile gas via the shared ADR-0014/0015 weight table.
pub(crate) fn hash_gas(weight: HostCallWeight) -> u64 {
    weight.gas()
}

/// Route a call to the precompile at `callee`.
///
/// `callee` is the string address form used across the codebase (`"0x01"`
/// .. `"0x07"`, see `strangecoin_core::address::is_precompile_address`).
/// Unknown addresses are [`PrecompileError::InvalidInput`] — the executor
/// only dispatches after the reserved-range check, so this is defense in
/// depth, not the primary gate.
pub fn dispatch_precompile(callee: &str, data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
    match callee {
        "0x01" => ecrecover::run(data),
        "0x02" => sha256::run(data),
        "0x03" => keccak::run(data),
        "0x04" => blake3::run(data),
        "0x05" => ed25519_verify::run(data),
        "0x06" => bls_aggregate::run(data),
        "0x07" => modexp::run(data),
        other => Err(PrecompileError::invalid_input(format!(
            "unknown precompile address `{other}`"
        ))),
    }
}
