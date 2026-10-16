//! Per-execution host state threaded through the wasmi `Store` (S1.5-P10).
//!
//! Every host function reaches it via `Caller::data[_mut]`. It carries the
//! working copy of the [`ExecutionContext`], the contract code registry for
//! nested calls, the active call stack (reentrancy guard, S1.5-P13), the
//! last nested-call return data, and the logs emitted so far.

use std::collections::HashMap;
use std::rc::Rc;

use strangecoin_core::vm::{ExecutionContext, Log};

/// Mutable state shared by every host function of one top-level execution.
pub struct HostState {
    /// Working copy of the call frame's context: `storage_overlay` and
    /// `accounts` are mutated in place by storage / value-transfer host
    /// functions; nested calls derive child contexts from it.
    pub ctx: ExecutionContext,
    /// Contract code registry (bech32 address -> WASM bytecode), shared
    /// with the executor via `Rc` so nested `call`/`delegatecall`/
    /// `staticcall` can load the callee's module without deep-cloning the
    /// whole registry per frame.
    pub code: Rc<HashMap<String, Vec<u8>>>,
    /// Active call stack (bottom = outermost frame's callee). Pushed by the
    /// call host functions before the child frame starts; consulted by the
    /// reentrancy guard (S1.5-P13).
    pub call_stack: Vec<String>,
    /// Return data of the most recent nested call, retained for the
    /// contract even when the out-buffer could not hold it all.
    pub last_return_data: Vec<u8>,
    /// Logs emitted by `emit_log` in this frame; merged into
    /// `ExecutionResult.logs` on success (S1.5-P14 formalizes receipts).
    pub logs: Vec<Log>,
}

impl HostState {
    /// `call_stack` is the full stack including this frame's callee
    /// (outermost first).
    pub fn new(
        ctx: ExecutionContext,
        code: Rc<HashMap<String, Vec<u8>>>,
        call_stack: Vec<String>,
    ) -> Self {
        Self {
            ctx,
            code,
            call_stack,
            last_return_data: Vec::new(),
            logs: Vec::new(),
        }
    }
}
