use std::collections::HashMap;
use std::rc::Rc;

use strangecoin_core::abi::ContractAbi;
use strangecoin_core::address::is_precompile_address;
use strangecoin_core::types::AccountState;
use strangecoin_core::vm::limits::{MAX_CALL_DEPTH, MAX_STACK_HEIGHT};
use strangecoin_core::vm::{
    ExecutionContext, ExecutionResult, Log, Opcode, TxStatus, ValueTransfer, VmError, VmExecutor,
};
use wasmi::{Config, Engine, ExternType, Instance, Linker, Module, Store};

mod error;
pub mod gas;
pub mod host;
pub mod precompiles;

pub use error::WasmiError;
pub use gas::{FuelGasCounter, HostCallWeight};
pub use host::HostState;
pub use precompiles::{dispatch_precompile, PrecompileError, PrecompileOutput};

/// Legacy fallback export invoked when no ABI selector matches the calldata
/// (S1.5-FBP03): contracts without a registered ABI, or called with fewer
/// than 4 calldata bytes, keep the pre-ABI single-entry convention.
const MAIN_EXPORT: &str = "main";

/// Deploy-time export invoked by [`FrameEntry::Constructor`]
/// (S1.5-FBP04, ADR-0013 amended): absent = legacy deploy without init.
const CONSTRUCTOR_EXPORT: &str = "constructor";

/// Page size of WASM linear memory, used when staging calldata for
/// selector-dispatched exports (S1.5-FBP03).
const WASM_PAGE_BYTES: usize = 65_536;

/// Which export a frame invokes (S1.5-FBP03/FBP04, ADR-0013 amended).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameEntry {
    /// Calldata selector dispatch with `main()` fallback — every `Call`
    /// frame and nested `call`/`staticcall`/`delegatecall` child.
    Selector,
    /// Deploy constructors: invoke the `"constructor"` export with the
    /// `init_args` calldata; a module without that export skips init
    /// (legacy compat, Success with the instantiation fuel only).
    Constructor,
}

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
        Rc::make_mut(&mut self.code).insert(address.to_string(), wasm);
    }

    /// Register the ABI of a contract (S1.5-P13: the `non_reentrant` flag
    /// consulted by the reentrancy guard; S1.5-FBP03: selector-based
    /// dispatch of calldata to WASM exports).
    pub fn insert_abi(&mut self, address: &str, abi: ContractAbi) {
        Rc::make_mut(&mut self.abis).insert(address.to_string(), abi);
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
            FrameEntry::Selector,
        )
        .map(|outcome| outcome.result)
    }

    /// Run the precompile at `ctx.callee` with `ctx.input` as the payload.
    ///
    /// ADR-0015 failure semantics: success returns the declared gas;
    /// `NotImplemented` reverts after charging the precompile's declared
    /// gas; `InvalidInput` reverts after burning the full call-frame gas.
    ///
    /// S1.5-FBP05: a top-level `value > 0` is recorded in the result's
    /// audit trail (precompiles never touch the account view; `apply_call`
    /// commits the trail on `Success`). Reachable only when a precompile
    /// address carries a contract account — plain precompile calls take
    /// the transfer-only path in `apply_call`.
    fn execute_precompile(&self, ctx: &ExecutionContext) -> Result<ExecutionResult, WasmiError> {
        let entry_transfer = || {
            (ctx.value > 0)
                .then(|| ValueTransfer {
                    from: ctx.caller.clone(),
                    to: ctx.callee.clone(),
                    amount: ctx.value,
                })
                .into_iter()
                .collect::<Vec<_>>()
        };
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
                    value_transfers: entry_transfer(),
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
                    value_transfers: Vec::new(),
                })
            }
            Err(PrecompileError::InvalidInput(_)) => Ok(ExecutionResult {
                gas_used: ctx.gas_limit,
                logs: Vec::new(),
                status: TxStatus::Reverted,
                return_data: Vec::new(),
                storage_overlay: HashMap::new(),
                value_transfers: Vec::new(),
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
            FrameEntry::Selector,
        )
        .map(|outcome| outcome.result)
    }

    fn execute_constructor(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
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
            FrameEntry::Constructor,
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
/// `entry` selects the export-resolution strategy (S1.5-FBP03 selector
/// dispatch vs S1.5-FBP04 constructor; ADR-0013 amended).
///
/// Resource limits (S1.5-P13): the engine caps the value stack at
/// [`MAX_STACK_HEIGHT`] bytes and wasm-internal recursion at
/// [`MAX_CALL_DEPTH`] frames (both trap as out-of-memory); the store's
/// resource limiter caps linear memory at `MAX_MEMORY_BYTES` and traps
/// `memory.grow` past it.
pub(crate) fn run_frame(
    wasm: &[u8],
    mut ctx: ExecutionContext,
    code: Rc<HashMap<String, Vec<u8>>>,
    abis: Rc<HashMap<String, ContractAbi>>,
    call_stack: Vec<String>,
    entry: FrameEntry,
) -> Result<FrameOutcome, WasmiError> {
    let gas_limit = ctx.gas_limit;

    // Selector-based dispatch (S1.5-FBP03, ADR-0013 amended): calldata of
    // at least 4 bytes is matched against the callee's registered ABI; a
    // hit names the WASM export to invoke, a miss falls back to the legacy
    // `main` export. Resolved before the store consumes `ctx`/`abis`.
    // Constructor frames resolve after instantiation (export presence
    // check, S1.5-FBP04).
    let dispatch_entry = match entry {
        FrameEntry::Selector => resolve_dispatch_entry(&abis, &ctx),
        FrameEntry::Constructor => None,
    };

    // Top-level value settlement (S1.5-FBP05, ADR-0013 amended): the entry
    // transfer credits the callee inside the frame's account view **before**
    // its code runs (withdraw pattern — the contract can forward what it
    // just received) and seeds the execution's audit trail. Nested frames
    // arrive with their transfer already settled by `nested_call`
    // (depth ≥ 1); constructor frames keep the FBP04 convention — the
    // endowment is credited by `apply_create` before the frame starts.
    let mut seeded_transfers = Vec::new();
    if entry == FrameEntry::Selector && ctx.depth == 0 && ctx.value > 0 {
        if !host::calls::transfer_value(&mut ctx.accounts, &ctx.caller, &ctx.callee, ctx.value) {
            // `apply_call` pre-checks the sender; a direct `execute_wasm`
            // caller may not. Fail the frame exceptionally, moving nothing.
            return Ok(FrameOutcome {
                result: ExecutionResult {
                    gas_used: gas_limit,
                    logs: Vec::new(),
                    status: TxStatus::Failed,
                    return_data: Vec::new(),
                    storage_overlay: HashMap::new(),
                    value_transfers: Vec::new(),
                },
                storage_overlay: HashMap::new(),
                accounts: ctx.accounts,
                logs: Vec::new(),
                return_data: Vec::new(),
            });
        }
        seeded_transfers.push(ValueTransfer {
            from: ctx.caller.clone(),
            to: ctx.callee.clone(),
            amount: ctx.value,
        });
    }

    let mut config = Config::default();
    config.consume_fuel(true);
    config.set_max_stack_height(MAX_STACK_HEIGHT);
    config.set_max_recursion_depth(MAX_CALL_DEPTH as usize);
    let engine = Engine::new(&config);

    let module = Module::new(&engine, wasm)
        .map_err(|e| WasmiError::Engine(format!("module decode: {e}")))?;

    // Constructor without an init export (S1.5-FBP04): skip before
    // instantiation — legacy modules (including oversized memories the
    // resource limiter would deny at instantiate time) keep the
    // deploy-without-init semantics, and the skip burns no fuel.
    if entry == FrameEntry::Constructor
        && !module.exports().any(|export| {
            export.name() == CONSTRUCTOR_EXPORT && matches!(export.ty(), ExternType::Func(_))
        })
    {
        return Ok(FrameOutcome {
            result: ExecutionResult {
                gas_used: 0,
                logs: Vec::new(),
                status: TxStatus::Success,
                return_data: Vec::new(),
                storage_overlay: HashMap::new(),
                value_transfers: Vec::new(),
            },
            storage_overlay: HashMap::new(),
            accounts: ctx.accounts,
            logs: Vec::new(),
            return_data: Vec::new(),
        });
    }

    let mut store = Store::new(&engine, HostState::new(ctx, code, abis, call_stack));
    store.data_mut().value_transfers = seeded_transfers;
    store.limiter(|state: &mut HostState| -> &mut dyn wasmi::ResourceLimiter { &mut state.limits });
    store
        .set_fuel(gas_limit)
        .map_err(|e| WasmiError::Engine(format!("set_fuel: {e}")))?;

    let mut linker = <Linker<HostState>>::new(&engine);
    host::register(&mut linker)
        .map_err(|e| WasmiError::Engine(format!("host registration: {e}")))?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| WasmiError::Instantiate(e.to_string()))?;

    let return_data = match entry {
        FrameEntry::Selector => match dispatch_entry {
            // ABI hit: the contract must export the entry — a missing export is
            // a contract bug, surfaced as `MissingExport` (no silent fallback).
            Some(name) => invoke_export(&instance, &mut store, &name, true)?,
            None => invoke_export(&instance, &mut store, MAIN_EXPORT, false)?,
        },
        // The module-level check above guarantees the export exists.
        FrameEntry::Constructor => invoke_export(&instance, &mut store, CONSTRUCTOR_EXPORT, true)?,
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
            value_transfers: state.value_transfers.clone(),
        },
        storage_overlay: state.ctx.storage_overlay,
        accounts: state.ctx.accounts,
        logs: state.logs,
        return_data,
    })
}

/// Resolve the ABI entry a frame's calldata selects (S1.5-FBP03).
///
/// Returns the export name of the matched entry, or `None` when the frame
/// has fewer than 4 calldata bytes or the callee has no registered ABI /
/// no entry for the selector — the caller falls back to the legacy `main`
/// export in that case.
fn resolve_dispatch_entry(
    abis: &HashMap<String, ContractAbi>,
    ctx: &ExecutionContext,
) -> Option<String> {
    let selector: [u8; 4] = ctx.input.get(..4)?.try_into().ok()?;
    abis.get(&ctx.callee)
        .and_then(|abi| abi.entry_by_selector(selector))
        .map(|entry| entry.name.clone())
}

/// Invoke `name` on `instance`, threading this frame's calldata when the
/// export takes the `(ptr, len)` argument pair (S1.5-FBP03 calling
/// convention, ADR-0013 amended).
///
/// Signature probe order: `(i32, i32) -> i32`, `(i32, i32) -> ()`,
/// `() -> i32`, `() -> ()` — so a legacy zero-argument export can still be
/// reached through the selector dispatch (calldata via `sc::input_read`).
/// An absent export is [`WasmiError::MissingExport`]; an export whose
/// signature is none of the four is [`WasmiError::TypeMismatch`].
fn invoke_export(
    instance: &Instance,
    store: &mut Store<HostState>,
    name: &str,
    with_calldata: bool,
) -> Result<Vec<u8>, WasmiError> {
    let Some(func) = instance.get_func(&store, name) else {
        return Err(WasmiError::MissingExport(name.to_string()));
    };
    if with_calldata {
        if let Ok(f) = func.typed::<(i32, i32), i32>(&*store) {
            let (ptr, len) = stage_calldata(instance, store)?;
            let value = f.call(&mut *store, (ptr, len)).map_err(WasmiError::from)?;
            return Ok(value.to_le_bytes().to_vec());
        }
        if let Ok(f) = func.typed::<(i32, i32), ()>(&*store) {
            let (ptr, len) = stage_calldata(instance, store)?;
            f.call(&mut *store, (ptr, len)).map_err(WasmiError::from)?;
            return Ok(Vec::new());
        }
    }
    if let Ok(f) = func.typed::<(), i32>(&*store) {
        let value = f.call(&mut *store, ()).map_err(WasmiError::from)?;
        return Ok(value.to_le_bytes().to_vec());
    }
    if let Ok(f) = func.typed::<(), ()>(&*store) {
        f.call(&mut *store, ()).map_err(WasmiError::from)?;
        return Ok(Vec::new());
    }
    Err(WasmiError::TypeMismatch(format!(
        "export `{name}` has no supported signature"
    )))
}

/// Copy the frame's calldata into a fresh tail of the module's linear
/// memory; returns the `(ptr, len)` pair to pass to a `(i32, i32)` export
/// (S1.5-FBP03).
///
/// The copy itself is unmetered: calldata is bounded by the transaction
/// size already validated by consensus. A grow denied by the store's
/// resource limiter surfaces as [`VmError::OutOfMemory`].
fn stage_calldata(
    instance: &Instance,
    store: &mut Store<HostState>,
) -> Result<(i32, i32), WasmiError> {
    let data_len = store.data().ctx.input.len();
    if data_len == 0 {
        return Ok((0, 0));
    }
    let memory = instance.get_memory(&store, "memory").ok_or_else(|| {
        WasmiError::Engine("host ABI: module does not export `memory`".to_string())
    })?;
    let old_size = memory.data_size(&store);
    let new_size = old_size
        .checked_add(data_len)
        .ok_or(WasmiError::Core(VmError::OutOfMemory))?;
    let old_pages = old_size.div_ceil(WASM_PAGE_BYTES);
    let new_pages = new_size.div_ceil(WASM_PAGE_BYTES);
    if new_pages > old_pages {
        memory
            .grow(&mut *store, (new_pages - old_pages) as u64)
            .map_err(|_| WasmiError::Core(VmError::OutOfMemory))?;
    }
    let data = store.data().ctx.input.clone();
    memory
        .write(&mut *store, old_size, &data)
        .map_err(|e| WasmiError::Engine(format!("calldata staging: {e}")))?;
    let ptr = i32::try_from(old_size).map_err(|_| WasmiError::Core(VmError::OutOfMemory))?;
    let len = i32::try_from(data_len).map_err(|_| WasmiError::Core(VmError::OutOfMemory))?;
    Ok((ptr, len))
}
