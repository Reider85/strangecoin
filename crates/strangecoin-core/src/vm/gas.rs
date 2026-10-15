use crate::vm::error::VmError;

/// Fixed per-block gas budget (ADR-0014, invariant #15).
///
/// Consensus rule: the sum of `gas_limit` over `Call` transactions in one
/// block may not exceed this value. Lives in core (runtime-free) so block
/// validation enforces it regardless of the `vm-wasm` feature. After mainnet
/// freeze any change goes through SCIP + activation height.
pub const BLOCK_GAS_LIMIT: u64 = 1_000_000;

/// Deterministic gas accounting for one execution.
///
/// Implementations live outside `strangecoin-core` (the wasmi-backed counter
/// ships with `strangecoin-vm-wasm`, S1.5-P09); core only fixes the
/// interface so metering logic stays testable without a runtime.
pub trait GasCounter {
    /// Charge `gas`, failing with [`VmError::OutOfGas`] when the remaining
    /// budget cannot cover it.
    fn spend(&mut self, gas: u64) -> Result<(), VmError>;

    /// Remaining gas in the current frame.
    fn gas_left(&self) -> u64;

    /// Gas consumed so far in the current frame.
    fn gas_used(&self) -> u64;
}
