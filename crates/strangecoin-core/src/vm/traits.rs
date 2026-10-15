use crate::vm::context::ExecutionContext;
use crate::vm::error::VmError;
use crate::vm::result::ExecutionResult;

/// Execution surface for smart contracts (Stage 1.5+).
///
/// The core crate depends on this trait, never on a concrete VM runtime.
/// Implementations (wasmi, etc.) live outside `strangecoin-core`.
pub trait VmExecutor {
    type Error: Into<VmError>;

    fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error>;

    /// Gas cost of one metering unit (ADR-0014: instruction / host-call /
    /// memory-growth weights).
    fn gas_cost(&self, opcode: Opcode) -> u64;
}

/// Placeholder metering categories — the real instruction set is wasmi's
/// (ADR-0012: no hand-written opcode table); this enum only names the
/// cost domains [`VmExecutor::gas_cost`] prices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Opcode {
    /// A regular executed WASM instruction (fuel).
    Instruction,
    /// A host function invocation (per-call weight).
    HostCall,
    /// A `memory.grow` of one 64KiB page.
    MemoryGrow,
    /// Anything else the engine bills (traps, instantiation, …).
    Other,
}
