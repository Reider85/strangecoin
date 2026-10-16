use std::collections::HashMap;
use std::rc::Rc;

use strangecoin_core::abi::ContractAbi;
use strangecoin_core::address::is_precompile_address;
use strangecoin_core::types::AccountState;
use strangecoin_core::vm::limits::{MAX_CALL_DEPTH, MAX_STACK_HEIGHT};
use strangecoin_core::vm::{
    ExecutionContext, ExecutionResult, Log, Opcode, TxStatus, VmError, VmExecutor,
};
use wasmi::{Config, Engine, Linker, Module, Store};

mod error;
pub mod gas;
pub mod host;
pub mod precompiles;

pub use error::WasmiError;
pub use gas::{FuelGasCounter, HostCallWeight};
pub use host::HostState;
pub use precompiles::{dispatch_precompile, PrecompileError, PrecompileOutput};

/// Contract entry point invoked by [`WasmiExecutor::execute`].
///
/// Placeholder until the JSON ABI fixes the real calling convention
/// (ADR-0013, S1.5-P12).
const ENTRY_POINT: &str = "main";

/// Outcome of one call frame: the [`ExecutionResult`] plus the frame's
/// mutated working state, so a nested call can merge the child's storage
/// overlay, accounts, and logs back into the parent on success (S1.5-P10).
#[derive(Debug)]
pub(crate) struct FrameOutcome {
    pub result: ExecutionResult,
    pub storage_overlay: HashMap<String, HashMap<[u8; 32], Vec<u8>>>,
    pub accounts: HashMap<String, AccountState>,
    pub logs: Vec<Log>,
    pub return_data: Vec<u8>,
}

/// Wasmi-backed implementation of [`VmExecutor`] (ADR-0012).
///
/// Metering (ADR-0014): the engine runs with `Config::consume_fuel(true)` on
/// the pinned wasmi 2.0.0 — fuel is deducted continuously as operators
/// execute (1 per substantive operator, 0 for Nop/Drop/control markers;
/// `BASE_INSTRUCTION_GAS = 1`). Host-call weights are charged manually via
/// [`gas::charge_host_call`]. Host functions register under the `sc`
/// namespace (S1.5-P10); precompiles (S1.5-P11) dispatch in
/// [`WasmiExecutor::execute`] before WASM instantiation when the callee is a
/// reserved address `0x01..=0x07`.
#[derive(Default)]
pub struct WasmiExecutor {
    /// Contract code registry: bech32 address -> WASM bytecode. Shared with
    /// nested frames via `Rc` (one clone per top-level execution, zero per
    /// nested call).
    code: Rc<HashMap<String, Vec<u8>>>,
    /// Contract ABI registry: bech32 address -> [`ContractAbi`]. Consulted
    /// by the reentrancy guard before nested calls (S1.5-P13); shared with
    /// nested frames like `code`.
    abis: Rc<HashMap<String, ContractAbi>>,
}

impl WasmiExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register WASM bytecode for a contract address.
    pub fn insert_code(&mut self, address: &str, wasm: Vec<u8>) {
        Rc::make_mut(&mut self.code)
            .insert(address.to_string(), wasm);
    }

    /// Register the ABI of a contract (S1.5-P13: the `non_reentrant` flag
    /// consulted by the reentrancy guard; full dispatch arrives with
    /// S1.5-P12).
    pub fn insert_abi(&mut self, address: &str, abi: ContractAbi) {
        Rc::make_mut(&mut self.abis)
            .insert(address.to_string(), abi);
    }

    /// Execute raw WASM bytecode with `ctx.gas_limit` as the fuel budget.
    pub fn execute_wasm(
        &self,
        wasm: &[u8],
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, WasmiError> {
        let call_stack = vec![ctx.callee.clone()];
        run_frame(
            wasm,
            ctx.clone(),
            Rc::clone(&self.code),
            Rc::clone(&self.abis),
            call_stack,
        )
        .map(|outcome| outcome.result)
    }

    /// Run the precompile at `ctx.callee` with `ctx.input` as the payload.
    ///
    /// ADR-0015 failure semantics: success returns the declared gas;
    /// `NotImplemented` reverts after charging the precompile's declared
    /// gas; `InvalidInput` reverts after burning the full call-frame gas.
    fn execute_precompile(&self, ctx: &ExecutionContext) -> Result<ExecutionResult, WasmiError> {
        match precompiles::dispatch_precompile(&ctx.callee, &ctx.input) {
            Ok(out) => {
                if out.gas_used > ctx.gas_limit {
                    return Err(WasmiError::OutOfFuel);
                }
                Ok(ExecutionResult {
                    gas_used: out.gas_used,
                    logs: Vec::new(),
                    status: TxStatus::Success,
                    return_data: out.output,
                    storage_overlay: HashMap::new(),
                })
            }
            Err(PrecompileError::NotImplemented { gas_used, .. }) => {
                if gas_used > ctx.gas_limit {
                    return Err(WasmiError::OutOfFuel);
                }
                Ok(ExecutionResult {
                    gas_used,
                    logs: Vec::new(),
                    status: TxStatus::Reverted,
                    return_data: Vec::new(),
                    storage_overlay: HashMap::new(),
                })
            }
            Err(PrecompileError::InvalidInput(_)) => Ok(ExecutionResult {
                gas_used: ctx.gas_limit,
                logs: Vec::new(),
                status: TxStatus::Reverted,
                return_data: Vec::new(),
                storage_overlay: HashMap::new(),
            }),
        }
    }
}

impl VmExecutor for WasmiExecutor {
    type Error = WasmiError;

    fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
        if is_precompile_address(&ctx.callee) {
            return self.execute_precompile(&ctx);
        }
        let wasm = self
            .code
            .get(&ctx.callee)
            .ok_or_else(|| WasmiError::NoCode {
                address: ctx.callee.clone(),
            })?;
        let call_stack = vec![ctx.callee.clone()];
        run_frame(
            wasm,
            ctx,
            Rc::clone(&self.code),
            Rc::clone(&self.abis),
            call_stack,
        )
        .map(|outcome| outcome.result)
    }

    fn gas_cost(&self, opcode: Opcode) -> u64 {
        match opcode {
            Opcode::Instruction => 1,
            Opcode::HostCall => 1,
            Opcode::MemoryGrow => 3,
            Opcode::Other => 1,
        }
    }

    fn register_code(&mut self, address: &str, wasm: Vec<u8>) -> Result<(), VmError> {
        let engine = Engine::default();
        Module::new(&engine, &wasm).map_err(|e| VmError::HostFunctionError {
            name: "register_code".to_string(),
            reason: format!("invalid wasm for `{address}`: {e}"),
        })?;
        self.insert_code(address, wasm);
        Ok(())
    }
}

/// Execute one call frame in a fresh engine/store (S1.5-P10).
///
/// Used both by the top-level executor and by the nested-call host functions
/// (`call`/`delegatecall`/`staticcall`), which pass a child
/// [`ExecutionContext`] derived from the parent's host state. Returns the
/// full [`FrameOutcome`] so the caller can merge storage/accounts/logs on
/// success. `call_stack` is the active stack including this frame's callee.
///
/// Resource limits (S1.5-P13): the engine caps the value stack at
/// [`MAX_STACK_HEIGHT`] bytes and wasm-internal recursion at
/// [`MAX_CALL_DEPTH`] frames (both trap as out-of-memory); the store's
/// resource limiter caps linear memory at `MAX_MEMORY_BYTES` and traps
/// `memory.grow` past it.
pub(crate) fn run_frame(
    wasm: &[u8],
    ctx: ExecutionContext,
    code: Rc<HashMap<String, Vec<u8>>>,
    abis: Rc<HashMap<String, ContractAbi>>,
    call_stack: Vec<String>,
) -> Result<FrameOutcome, WasmiError> {
    let gas_limit = ctx.gas_limit;
    let mut config = Config::default();
    config.consume_fuel(true);
    config.set_max_stack_height(MAX_STACK_HEIGHT);
    config.set_max_recursion_depth(MAX_CALL_DEPTH as usize);
    let engine = Engine::new(&config);

    let module = Module::new(&engine, wasm)
        .map_err(|e| WasmiError::Engine(format!("module decode: {e}")))?;

    let mut store = Store::new(
        &engine,
        HostState::new(ctx, code, abis, call_stack),
    );
    store.limiter(|state: &mut HostState| -> &mut dyn wasmi::ResourceLimiter {
        &mut state.limits
    });
    store
        .set_fuel(gas_limit)
        .map_err(|e| WasmiError::Engine(format!("set_fuel: {e}")))?;

    let mut linker = <Linker<HostState>>::new(&engine);
    host::register(&mut linker)
        .map_err(|e| WasmiError::Engine(format!("host registration: {e}")))?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| WasmiError::Instantiate(e.to_string()))?;

    let return_data = match instance.get_typed_func::<(), i32>(&store, ENTRY_POINT) {
        Ok(func) => {
            let value = func.call(&mut store, ()).map_err(WasmiError::from)?;
            value.to_le_bytes().to_vec()
        }
        Err(_) => {
            let func = instance
                .get_typed_func::<(), ()>(&store, ENTRY_POINT)
                .map_err(|_| WasmiError::MissingExport(ENTRY_POINT.to_string()))?;
            func.call(&mut store, ()).map_err(WasmiError::from)?;
            Vec::new()
        }
    };

    let fuel_left = store
        .get_fuel()
        .map_err(|e| WasmiError::Engine(format!("get_fuel: {e}")))?;
    let gas_used = gas_limit.saturating_sub(fuel_left);
    let state = store.into_data();

    Ok(FrameOutcome {
        result: ExecutionResult {
            gas_used,
            logs: state.logs.clone(),
            status: TxStatus::Success,
            return_data: return_data.clone(),
            storage_overlay: state.ctx.storage_overlay.clone(),
        },
        storage_overlay: state.ctx.storage_overlay,
        accounts: state.ctx.accounts,
        logs: state.logs,
        return_data,
    })
}
