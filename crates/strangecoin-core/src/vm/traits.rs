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

    /// Execute a deploy's constructor frame (S1.5-FBP04, ADR-0013 amended):
    /// `ctx.input` carries the `CreateTx.init_args` calldata and
    /// `ctx.gas_limit` the `CreateTx.gas_limit` budget. The default
    /// delegates to [`execute`](Self::execute); runtimes that distinguish
    /// constructors (wasmi: invoke the `"constructor"` export, skip when
    /// absent) override it.
    fn execute_constructor(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
        self.execute(ctx)
    }

    /// Gas cost of one metering unit (ADR-0014: instruction / host-call /
    /// memory-growth weights).
    fn gas_cost(&self, opcode: Opcode) -> u64;

    /// Register WASM bytecode for a contract address (S1.5-P14: `Create`
    /// installs code so a later `Call` in the same chain can execute it).
    /// Implementations must reject bytecode the engine cannot load.
    fn register_code(&mut self, address: &str, wasm: Vec<u8>) -> Result<(), VmError>;
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
