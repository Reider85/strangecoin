use std::collections::HashMap;

use strangecoin_core::vm::{ExecutionContext, ExecutionResult, Opcode, TxStatus, VmExecutor};
use wasmi::{Config, Engine, Linker, Module, Store};

mod error;

pub use error::WasmiError;

/// Contract entry point invoked by [`WasmiExecutor::execute`].
///
/// Placeholder until the JSON ABI fixes the real calling convention
/// (ADR-0013, S1.5-P12).
const ENTRY_POINT: &str = "main";

/// Wasmi-backed implementation of [`VmExecutor`] (ADR-0012).
///
/// Stage 1.5 scope is structural: load a module, instantiate it with fuel
/// metering enabled, call `main()`, report gas consumed. Host functions
/// (S1.5-P10), host-call weights (S1.5-P09) and precompiles (S1.5-P11) land
/// in follow-up prompts.
#[derive(Default)]
pub struct WasmiExecutor {
    /// Contract code registry: bech32 address -> WASM bytecode.
    ///
    /// `AccountState` carries no code yet (`Account` model lands in S1.5-P07);
    /// afterwards this registry moves to `code_hash` keys.
    code: HashMap<String, Vec<u8>>,
}

impl WasmiExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register WASM bytecode for a contract address.
    pub fn insert_code(&mut self, address: &str, wasm: Vec<u8>) {
        self.code.insert(address.to_string(), wasm);
    }

    /// Execute raw WASM bytecode with `ctx.gas_limit` as the fuel budget.
    pub fn execute_wasm(
        &self,
        wasm: &[u8],
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, WasmiError> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);

        let module = Module::new(&engine, wasm)
            .map_err(|e| WasmiError::Engine(format!("module decode: {e}")))?;

        let mut store = Store::new(&engine, ());
        store
            .set_fuel(ctx.gas_limit)
            .map_err(|e| WasmiError::Engine(format!("set_fuel: {e}")))?;

        let linker = <Linker<()>>::new(&engine);
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
        let gas_used = ctx.gas_limit.saturating_sub(fuel_left);

        Ok(ExecutionResult {
            gas_used,
            logs: Vec::new(),
            status: TxStatus::Success,
            return_data,
        })
    }
}

impl VmExecutor for WasmiExecutor {
    type Error = WasmiError;

    fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
        let wasm = self
            .code
            .get(&ctx.callee)
            .ok_or_else(|| WasmiError::NoCode {
                address: ctx.callee.clone(),
            })?;
        self.execute_wasm(wasm, &ctx)
    }

    fn gas_cost(&self, opcode: Opcode) -> u64 {
        match opcode {
            Opcode::Instruction => 1,
            Opcode::HostCall => 1,
            Opcode::MemoryGrow => 3,
            Opcode::Other => 1,
        }
    }
}
