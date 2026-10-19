use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use crate::address::derive_contract_address;
use crate::economics::emission::block_reward_at_height_for_chain;
use crate::error::CoreError;
use crate::serialize::txid;
use crate::state::contract_storage::{ContractStorageOverlay, SharedContractStorage};
use crate::state::sparse_merkle::contract_storage_root;
use crate::types::{Account, Block, ContractAccount, Transaction, TransferTx};
use crate::vm::{
    BlockContext, ExecutionContext, ExecutionResult, Opcode, Receipt, TxStatus, ValueTransfer,
    VmError, VmExecutor, BLOCK_GAS_LIMIT,
};

/// Executor stand-in for the transfer-only [`apply_block`] path: `Create`/
/// `Call` arms are never reached with `None`, so its methods are unreachable.
struct NoVm;

impl VmExecutor for NoVm {
    type Error = VmError;

    fn execute(&self, _ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
        Err(VmError::HostFunctionError {
            name: "apply_block".to_string(),
            reason: "no VM executor available".to_string(),
        })
    }

    fn gas_cost(&self, _opcode: Opcode) -> u64 {
        0
    }

    fn register_code(&mut self, _address: &str, _wasm: Vec<u8>) -> Result<(), VmError> {
        Err(VmError::HostFunctionError {
            name: "apply_block".to_string(),
            reason: "no VM executor available".to_string(),
        })
    }
}

/// Pre-image of one contract storage slot write (S1.5-P15): the contract
/// address, the slot key, and the value before the call (`None` = the key
/// was absent). Stored per txid in [`State::storage_undo`] so `unapply`
/// restores the exact pre-call contents.
pub type StorageUndoEntry = (String, [u8; 32], Option<Vec<u8>>);

/// Post-block chain state: account balances, per-contract storage, plus the
/// receipts and undo records of the most recently applied block
/// (S1.5-P14 invariant #18, S1.5-P15 reorg support).
///
/// `receipts`, `storage_undo` and `value_transfers_undo` are derived data —
/// a pure function of the applied block. `receipts` is replaced wholesale on
/// every apply and cleared on unapply; the two undo maps **accumulate**
/// (keyed by txid) so chained rollbacks of non-tip blocks still find the
/// pre-images they need. `PartialEq` compares balances **and**
/// `contract_storage`: the apply/unapply round-trip (invariant #4, on which
/// reorg correctness rests) must restore both; the storage contents are
/// additionally committed by `ContractAccount.storage_root` inside the
/// account leaves. Receipts of any height are recomputed from the chain and
/// stay outside equality.
///
/// `contract_storage` is a [`SharedContractStorage`] handle (S1.5-FBP06):
/// call frames share it via `Arc::clone` instead of deep-copying the map
/// per transaction. Cloning a `State` therefore **must** copy the map into
/// a fresh `Arc` (see the manual `Clone` impl) — sharing the handle would
/// let `apply_block`'s working copy mutate the caller's snapshot through
/// the shared base, breaking invariant #4.
#[derive(Debug, Default)]
pub struct State {
    pub balances: HashMap<String, Account>,
    /// Persisted contract storage keyed by contract address (S1.5-P15);
    /// each contract's map commits to `ContractAccount.storage_root`.
    pub contract_storage: SharedContractStorage,
    pub receipts: Vec<Receipt>,
    /// Undo record of the tip block's `Call` transactions (S1.5-P15):
    /// txid → pre-images of every storage slot the call wrote.
    pub storage_undo: HashMap<[u8; 32], Vec<StorageUndoEntry>>,
    /// Undo record of every committed value move, per transaction
    /// (S1.5-FBP05): txid → transfers in commit order. `unapply` reverses
    /// the list in reverse order (invariant #4); the same moves are mirrored
    /// into the transaction's `Receipt::value_transfers` for the audit
    /// trail. Accumulates across blocks like `storage_undo`.
    pub value_transfers_undo: HashMap<[u8; 32], Vec<ValueTransfer>>,
}

impl Clone for State {
    fn clone(&self) -> Self {
        Self {
            balances: self.balances.clone(),
            // S1.5-FBP06: deep-copy the storage map into a fresh Arc —
            // sharing the handle would alias the caller's snapshot.
            contract_storage: Arc::new(RwLock::new(
                self.contract_storage
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            )),
            receipts: self.receipts.clone(),
            storage_undo: self.storage_undo.clone(),
            value_transfers_undo: self.value_transfers_undo.clone(),
        }
    }
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        // S1.5-FBP06: compare the shared storage through its contents
        // (`RwLock` has no `PartialEq`).
        let storage_eq = {
            let a = self
                .contract_storage
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let b = other
                .contract_storage
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *a == *b
        };
        self.balances == other.balances && storage_eq
    }
}

impl Eq for State {}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_balance(&self, addr: &str) -> u64 {
        self.balances.get(addr).map(|a| a.balance()).unwrap_or(0)
    }

    pub fn get_nonce(&self, addr: &str) -> u64 {
        self.balances.get(addr).map(|a| a.nonce()).unwrap_or(0)
    }

    pub fn get_account(&self, addr: &str) -> Option<&Account> {
        self.balances.get(addr)
    }

    pub fn total_supply(&self) -> u64 {
        self.balances.values().map(|a| a.balance()).sum()
    }

    /// Set the balance, preserving the account variant: crediting or
    /// debiting a contract account must not turn it into an EOA (S1.5-P07).
    pub fn set_balance(&mut self, addr: &str, balance: u64) {
        self.balances
            .entry(addr.to_string())
            .or_default()
            .set_balance(balance);
        self.prune_if_empty(addr);
    }

    pub fn set_nonce(&mut self, addr: &str, nonce: u64) {
        self.balances
            .entry(addr.to_string())
            .or_default()
            .set_nonce(nonce);
        self.prune_if_empty(addr);
    }

    /// Install `account` at `addr` verbatim (contract deploy path, S1.5-P07);
    /// an empty account removes the entry.
    pub fn set_account(&mut self, addr: &str, account: Account) {
        if account.is_empty() {
            self.balances.remove(addr);
        } else {
            self.balances.insert(addr.to_string(), account);
        }
    }

    /// An empty account ([`Account::is_empty`]: EOA `(0, 0)`, phantom
    /// contract) carries no information: every read path defaults a missing
    /// account to the empty EOA. Keeping the state free of empty accounts is
    /// what makes `apply`/`unapply` an exact round-trip, which reorg and
    /// state-cache invalidation depend on. Deployed contract accounts are
    /// never empty — their code_hash pins them in the state.
    fn prune_if_empty(&mut self, addr: &str) {
        let empty = self
            .balances
            .get(addr)
            .is_some_and(|account| account.is_empty());
        if empty {
            self.balances.remove(addr);
        }
    }
}

/// Apply `block` on top of `state` without contract execution: `Create` and
/// `Call` transactions are rejected (light-node / feature-off path). Transfer
/// blocks still produce receipts (S1.5-P14). `chain_id` pins the network
/// (BUG-S1-004).
pub fn apply_block(state: &State, block: &Block, chain_id: u32) -> Result<State, CoreError> {
    apply_block_inner::<NoVm>(state, block, chain_id, None)
}

/// Apply `block` on top of `state`, executing `Create`/`Call` transactions
/// through `vm` (S1.5-P14). Receipts of every transaction land in
/// `state.receipts`; contract storage overlays are transient until S1.5-P15.
pub fn apply_block_with_vm<V: VmExecutor>(
    state: &State,
    block: &Block,
    chain_id: u32,
    vm: &mut V,
) -> Result<State, CoreError> {
    apply_block_inner(state, block, chain_id, Some(vm))
}

fn apply_block_inner<V: VmExecutor>(
    state: &State,
    block: &Block,
    chain_id: u32,
    vm: Option<&mut V>,
) -> Result<State, CoreError> {
    let mut new_state = state.clone();
    new_state.receipts.clear();
    // `storage_undo` intentionally accumulates: entries are keyed by txid
    // and consumed by `unapply_call_storage`, so a chained unapply (reorg
    // of several tip blocks) can still find the earlier blocks' pre-images.
    // Production never grows it — `StateCache::commit` drops the record
    // every block and reorgs replay via `rebuild_from_chain`.
    let mut receipts: Vec<Receipt> = Vec::with_capacity(block.transactions.len());

    if block.index == 0 {
        apply_genesis(&mut new_state, block, chain_id, &mut receipts)?;
    } else {
        apply_non_genesis(&mut new_state, block, chain_id, vm, &mut receipts)?;
    }

    new_state.receipts = receipts;
    Ok(new_state)
}

fn transfer_receipt(
    tx: &Transaction,
    block: &Block,
    tx_index: u32,
    value_transfers: Vec<ValueTransfer>,
) -> Receipt {
    Receipt {
        txid: txid(tx),
        gas_used: 0,
        logs: Vec::new(),
        status: TxStatus::Success,
        block_height: block.index,
        tx_index,
        value_transfers,
    }
}

fn apply_genesis(
    state: &mut State,
    block: &Block,
    chain_id: u32,
    receipts: &mut Vec<Receipt>,
) -> Result<(), CoreError> {
    for (tx_index, tx) in block.transactions.iter().enumerate() {
        validate_tx_chain_id(tx, chain_id)?;
        match tx {
            Transaction::Transfer(transfer) => {
                credit_receiver(state, transfer)?;
                // Genesis allocations are mints (credit-only, no payer)
                // — no value-transfer trail (S1.5-FBP05).
                receipts.push(transfer_receipt(tx, block, tx_index as u32, Vec::new()));
            }
            Transaction::Create(_) | Transaction::Call(_) => {
                return Err(CoreError::UnsupportedTxType {
                    reason: "create/call transactions are not supported at genesis".to_string(),
                })
            }
        }
    }
    Ok(())
}

fn validate_tx_chain_id(tx: &Transaction, chain_id: u32) -> Result<(), CoreError> {
    if tx.chain_id() != chain_id {
        return Err(CoreError::InvalidChainId {
            expected: chain_id,
            got: tx.chain_id(),
        });
    }
    Ok(())
}

fn apply_non_genesis<V: VmExecutor>(
    state: &mut State,
    block: &Block,
    chain_id: u32,
    mut vm: Option<&mut V>,
    receipts: &mut Vec<Receipt>,
) -> Result<(), CoreError> {
    for tx in &block.transactions {
        validate_tx_chain_id(tx, chain_id)?;
    }

    let total_supply = state.total_supply();
    let expected_reward = block_reward_at_height_for_chain(block.index, total_supply, chain_id);

    let coinbase_index = block
        .transactions
        .iter()
        .position(|tx| tx.is_coinbase())
        .ok_or(CoreError::InvalidCoinbaseAmount {
            expected: expected_reward,
            got: 0,
        })?;

    let coinbase_transfer = match &block.transactions[coinbase_index] {
        Transaction::Transfer(transfer) => transfer,
        _ => {
            return Err(CoreError::UnsupportedTxType {
                reason: "coinbase must be a transfer transaction".to_string(),
            })
        }
    };

    if coinbase_transfer.amount > expected_reward {
        return Err(CoreError::InvalidCoinbaseAmount {
            expected: expected_reward,
            got: coinbase_transfer.amount,
        });
    }

    credit_receiver(state, coinbase_transfer)?;
    receipts.push(transfer_receipt(
        &block.transactions[coinbase_index],
        block,
        coinbase_index as u32,
        // Coinbase is a mint, not a move between accounts (S1.5-FBP05).
        Vec::new(),
    ));

    for (tx_index, tx) in block.transactions.iter().enumerate() {
        if tx.is_coinbase() {
            continue;
        }
        match tx {
            Transaction::Transfer(transfer) => {
                deduct_sender(state, transfer)?;
                credit_receiver(state, transfer)?;
                increment_nonce(state, transfer)?;
                receipts.push(transfer_receipt(
                    tx,
                    block,
                    tx_index as u32,
                    vec![ValueTransfer {
                        from: transfer.from.clone(),
                        to: transfer.to.clone(),
                        amount: transfer.amount,
                    }],
                ));
            }
            Transaction::Create(create) => match vm.as_deref_mut() {
                None => {
                    return Err(CoreError::UnsupportedTxType {
                        reason: "create transactions require a VM executor (S1.5-P14)"
                            .to_string(),
                    })
                }
                Some(vm) => {
                    let receipt = apply_create(state, create, chain_id, vm, block, tx_index as u32)?;
                    receipts.push(receipt);
                }
            },
            Transaction::Call(call) => match vm.as_deref_mut() {
                None => {
                    return Err(CoreError::UnsupportedTxType {
                        reason: "call transactions require a VM executor (S1.5-P14)".to_string(),
                    })
                }
                Some(vm) => {
                    let receipt = apply_call(state, block, call, vm, tx_index as u32)?;
                    receipts.push(receipt);
                }
            },
        }
    }

    Ok(())
}

/// Deploy path (S1.5-P14, extended S1.5-FBP04): install the contract
/// account, register its code, then execute the constructor frame.
///
/// Constructor convention (ADR-0013 amended): the runtime invokes the WASM
/// export `"constructor"` with `init_args` as calldata under the
/// `CreateTx.gas_limit` budget; a module without that export deploys
/// without init (legacy compat). On `Success` the staged storage writes
/// are flushed (`storage_root` recomputed — invariant #19 extension), the
/// `endowment` plus every nested transfer the constructor settled are
/// committed (S1.5-FBP05, mirrored into the receipt's `value_transfers`),
/// and the receipt carries the metered gas and constructor logs. On failure
/// (out-of-gas, engine error, reverted frame) the deployment is rolled back
/// — no account, no storage, no endowment movement — the sender nonce stays
/// consumed, and the receipt records `OutOfGas`/`Failed` (failed-receipt
/// semantics, mirrors `apply_call`; the block still applies).
fn apply_create<V: VmExecutor>(
    state: &mut State,
    create: &crate::types::CreateTx,
    chain_id: u32,
    vm: &mut V,
    block: &Block,
    tx_index: u32,
) -> Result<Receipt, CoreError> {
    let sender_nonce = state.get_nonce(&create.from);
    let address = derive_contract_address(&create.from, sender_nonce, chain_id)?;
    if state
        .get_account(&address)
        .is_some_and(|account| account.is_contract())
    {
        return Err(CoreError::InvalidContractDeploy {
            reason: format!("contract address collision at `{address}`"),
        });
    }

    let sender_balance = state.get_balance(&create.from);
    if sender_balance < create.endowment {
        return Err(CoreError::InsufficientBalance {
            sender: create.from.clone(),
            available: sender_balance,
            required: create.endowment,
        });
    }

    // The endowment is credited to the contract account up-front (the
    // constructor may observe its own balance through the accounts view)
    // but only debited from the sender on `Success` — a failed frame
    // leaves both sides untouched.
    let code_hash = *blake3::hash(&create.init_code).as_bytes();
    state.set_account(
        &address,
        Account::Contract(ContractAccount {
            code_hash,
            storage_root: [0u8; 32],
            nonce: 1,
            balance: create.endowment,
        }),
    );
    state.set_nonce(&create.from, sender_nonce + 1);
    vm.register_code(&address, create.init_code.clone())?;

    let coinbase = block
        .transactions
        .iter()
        .find(|tx| tx.is_coinbase())
        .and_then(|tx| tx.receiver())
        .unwrap_or_default()
        .to_string();

    let ctx = ExecutionContext {
        block: block.header(),
        accounts: state.balances.clone(),
        caller: create.from.clone(),
        callee: address.clone(),
        value: create.endowment,
        gas_limit: create.gas_limit,
        gas_used: 0,
        depth: 0,
        reentrant: false,
        read_only: false,
        block_context: BlockContext {
            height: block.index,
            timestamp: block.timestamp,
            coinbase,
            block_gas_limit: BLOCK_GAS_LIMIT,
        },
        storage: Arc::clone(&state.contract_storage),
        storage_overlay: HashMap::new(),
        storage_address: address.clone(),
        input: create.init_args.clone(),
    };

    let create_tx = Transaction::Create(create.clone());
    let txid = txid(&create_tx);
    match vm.execute_constructor(ctx) {
        Ok(result) if result.status == TxStatus::Success => {
            flush_storage_writes(state, &result, txid);
            // The contract account was credited the endowment up-front (so
            // the constructor could observe its balance); `Success` debits
            // the sender to make the move final.
            let new_sender = state
                .get_balance(&create.from)
                .checked_sub(create.endowment)
                .ok_or(CoreError::StateOverflow)?;
            state.set_balance(&create.from, new_sender);
            // S1.5-FBP05: commit every nested transfer the constructor
            // settled (undo record for `unapply`); the receipt mirrors the
            // full trail — endowment move first, then the nested chain.
            commit_value_transfers(state, txid, &result.value_transfers)?;
            let mut value_transfers = Vec::with_capacity(result.value_transfers.len() + 1);
            if create.endowment > 0 {
                value_transfers.push(ValueTransfer {
                    from: create.from.clone(),
                    to: address.clone(),
                    amount: create.endowment,
                });
            }
            value_transfers.extend(result.value_transfers.iter().cloned());
            let mut receipt = Receipt::from_execution(txid, block.index, tx_index, &result);
            receipt.value_transfers = value_transfers;
            Ok(receipt)
        }
        Ok(result) => {
            // Defensive non-success status: roll the deployment back.
            state.set_account(&address, Account::default());
            Ok(Receipt::from_execution(
                txid,
                block.index,
                tx_index,
                &result,
            ))
        }
        Err(err) => {
            let vm_error: VmError = err.into();
            state.set_account(&address, Account::default());
            Ok(Receipt::from_vm_error(
                txid,
                block.index,
                tx_index,
                &vm_error,
                create.gas_limit,
            ))
        }
    }
}

/// Call path (S1.5-P14/P15, reworked S1.5-FBP05): execute the contract
/// entry point through `vm`. Value settlement (ADR-0013 amended): the
/// top-level frame's entry transfer and every nested `call` transfer settle
/// **inside** the VM against the frame's account view — the contract sees
/// the incoming value before its code runs (withdraw pattern) — and land in
/// `ExecutionResult::value_transfers`. On `Success` the whole trail is
/// committed to `state.balances` through `commit_value_transfers` (undo
/// record in `State::value_transfers_undo`); a failed frame (revert,
/// out-of-gas, engine error) moves nothing, still increments the sender
/// nonce and produces a receipt. A call to an EOA (no contract account) is
/// a plain value transfer with a zero-gas success receipt and the same
/// trail/undo treatment. The receipt mirrors the trail
/// (`Receipt::value_transfers`).
///
/// Contract storage (S1.5-P15): the frame sees the chain-wide storage
/// snapshot in `ctx.storage` (reads fall through to it) and stages writes
/// in the scoped overlay; on `Success` every contract scope of
/// `ExecutionResult::storage_overlay` is flushed into
/// `State.contract_storage`, each touched contract's `storage_root` is
/// recomputed into its account record (invariant #19 extension), and the
/// pre-images land in `State::storage_undo` for `unapply`. Failed frames
/// discard their writes.
fn apply_call<V: VmExecutor>(
    state: &mut State,
    block: &Block,
    call: &crate::types::CallTx,
    vm: &mut V,
    tx_index: u32,
) -> Result<Receipt, CoreError> {
    let sender_balance = state.get_balance(&call.from);
    if sender_balance < call.value {
        return Err(CoreError::InsufficientBalance {
            sender: call.from.clone(),
            available: sender_balance,
            required: call.value,
        });
    }
    state.set_nonce(&call.from, state.get_nonce(&call.from) + 1);

    let call_tx = Transaction::Call(call.clone());
    let txid = txid(&call_tx);

    let is_contract = state
        .get_account(&call.to)
        .is_some_and(|account| account.is_contract());
    if !is_contract {
        let value_transfers = if call.value > 0 {
            vec![ValueTransfer {
                from: call.from.clone(),
                to: call.to.clone(),
                amount: call.value,
            }]
        } else {
            Vec::new()
        };
        commit_value_transfers(state, txid, &value_transfers)?;
        return Ok(Receipt {
            txid,
            gas_used: 0,
            logs: Vec::new(),
            status: TxStatus::Success,
            block_height: block.index,
            tx_index,
            value_transfers,
        });
    }

    let coinbase = block
        .transactions
        .iter()
        .find(|tx| tx.is_coinbase())
        .and_then(|tx| tx.receiver())
        .unwrap_or_default()
        .to_string();

    let ctx = ExecutionContext {
        block: block.header(),
        accounts: state.balances.clone(),
        caller: call.from.clone(),
        callee: call.to.clone(),
        value: call.value,
        gas_limit: call.gas_limit,
        gas_used: 0,
        depth: 0,
        reentrant: false,
        read_only: false,
        block_context: BlockContext {
            height: block.index,
            timestamp: block.timestamp,
            coinbase,
            block_gas_limit: BLOCK_GAS_LIMIT,
        },
        storage: Arc::clone(&state.contract_storage),
        storage_overlay: HashMap::new(),
        storage_address: call.to.clone(),
        input: call.data.clone(),
    };

    match vm.execute(ctx) {
        Ok(result) => {
            if result.status == TxStatus::Success {
                commit_value_transfers(state, txid, &result.value_transfers)?;
                flush_storage_writes(state, &result, txid);
            }
            Ok(Receipt::from_execution(txid, block.index, tx_index, &result))
        }
        Err(err) => {
            let vm_error: VmError = err.into();
            Ok(Receipt::from_vm_error(
                txid,
                block.index,
                tx_index,
                &vm_error,
                call.gas_limit,
            ))
        }
    }
}

/// Flush a successful frame's staged contract storage writes into
/// `state.contract_storage` (S1.5-P15, S1.5-FBP06: in-place merge through
/// the shared handle under a single `write()` guard — the base is not
/// cloned): every contract scope whose account record is a contract gets
/// its writes merged, its `storage_root` recomputed into the account leaf
/// (invariant #19 extension), and its pre-images recorded under `txid` in
/// `State::storage_undo` so `unapply` can restore the exact prior
/// contents. Scopes without a contract account are ignored — only
/// contracts have storage.
fn flush_storage_writes(state: &mut State, result: &ExecutionResult, txid: [u8; 32]) {
    let mut undo: Vec<StorageUndoEntry> = Vec::new();
    let mut storage_map = state
        .contract_storage
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (address, writes) in &result.storage_overlay {
        if writes.is_empty() {
            continue;
        }
        let is_contract = state
            .get_account(address)
            .is_some_and(|account| account.is_contract());
        if !is_contract {
            continue;
        }
        let overlay = ContractStorageOverlay::from_writes(writes);
        let storage = storage_map.entry(address.clone()).or_default();
        for (key, preimage) in overlay.preimages_of(storage) {
            undo.push((address.clone(), key, preimage));
        }
        overlay.apply_to(storage);
        let new_root = contract_storage_root(storage);
        if let Some(Account::Contract(account)) = state.balances.get_mut(address) {
            account.storage_root = new_root;
        }
    }
    drop(storage_map);
    if !undo.is_empty() {
        state.storage_undo.insert(txid, undo);
    }
}

/// Commit `transfers` to `state.balances` (S1.5-FBP05): each move deducts
/// the payer and credits the payee with checked arithmetic, reading live
/// balances sequentially so a self-move (`from == to`) nets to zero. The
/// list is recorded under `txid` in [`State::value_transfers_undo`] so
/// `unapply` can reverse it in reverse commit order (invariant #4). Empty
/// lists write no undo record.
fn commit_value_transfers(
    state: &mut State,
    txid: [u8; 32],
    transfers: &[ValueTransfer],
) -> Result<(), CoreError> {
    if transfers.is_empty() {
        return Ok(());
    }
    for transfer in transfers {
        let payer = state.get_balance(&transfer.from);
        let new_payer = payer
            .checked_sub(transfer.amount)
            .ok_or(CoreError::StateOverflow)?;
        state.set_balance(&transfer.from, new_payer);
        let payee = state.get_balance(&transfer.to);
        let new_payee = payee
            .checked_add(transfer.amount)
            .ok_or(CoreError::StateOverflow)?;
        state.set_balance(&transfer.to, new_payee);
    }
    state
        .value_transfers_undo
        .insert(txid, transfers.to_vec());
    Ok(())
}

/// Reverse previously committed value moves (S1.5-FBP05): undo each transfer
/// in reverse commit order — deduct the payee, credit the payer — with the
/// same sequential live-balance reads as [`commit_value_transfers`], so
/// self-moves stay balanced and nested chains unwind in exact mirror order.
fn reverse_value_transfers(
    state: &mut State,
    transfers: &[ValueTransfer],
) -> Result<(), CoreError> {
    for transfer in transfers.iter().rev() {
        let payee = state.get_balance(&transfer.to);
        let new_payee = payee
            .checked_sub(transfer.amount)
            .ok_or(CoreError::StateOverflow)?;
        state.set_balance(&transfer.to, new_payee);
        let payer = state.get_balance(&transfer.from);
        let new_payer = payer
            .checked_add(transfer.amount)
            .ok_or(CoreError::StateOverflow)?;
        state.set_balance(&transfer.from, new_payer);
    }
    Ok(())
}

fn deduct_sender(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let sender_balance = state.get_balance(&tx.from);
    if sender_balance < tx.amount {
        return Err(CoreError::InsufficientBalance {
            sender: tx.from.clone(),
            available: sender_balance,
            required: tx.amount,
        });
    }
    let new_balance = sender_balance
        .checked_sub(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.from, new_balance);
    Ok(())
}

fn credit_receiver(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let receiver_balance = state.get_balance(&tx.to);
    let new_balance = receiver_balance
        .checked_add(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.to, new_balance);
    Ok(())
}

fn increment_nonce(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let nonce = state.get_nonce(&tx.from);
    state.set_nonce(&tx.from, nonce + 1);
    Ok(())
}

/// Reverse `block` on top of `state` (S1.5-P14: also reverses `Create` and
/// `Call`, then clears the tip receipts). `chain_id` is required to re-derive
/// contract addresses on the way back.
pub fn unapply_block(
    state: &State,
    block: &Block,
    chain_id: u32,
) -> Result<State, CoreError> {
    let mut new_state = state.clone();

    if block.index == 0 {
        unapply_genesis(&mut new_state, block)?;
    } else {
        unapply_non_genesis(&mut new_state, block, chain_id)?;
    }

    // Receipts are cleared; `storage_undo` and `value_transfers_undo`
    // entries of the unapplied block were consumed per txid, entries of
    // earlier blocks stay for chained rollbacks.
    new_state.receipts.clear();
    Ok(new_state)
}

fn unapply_genesis(state: &mut State, block: &Block) -> Result<(), CoreError> {
    for tx in block.transactions.iter().rev() {
        match tx {
            Transaction::Transfer(transfer) => debit_receiver(state, transfer)?,
            Transaction::Create(_) | Transaction::Call(_) => {
                return Err(CoreError::UnsupportedTxType {
                    reason: "create/call transactions are not supported at genesis".to_string(),
                })
            }
        }
    }
    Ok(())
}

fn unapply_non_genesis(state: &mut State, block: &Block, chain_id: u32) -> Result<(), CoreError> {
    for tx in block.transactions.iter().rev() {
        match tx {
            Transaction::Transfer(transfer) => {
                if transfer.is_coinbase {
                    debit_receiver(state, transfer)?;
                    continue;
                }
                decrement_nonce(state, transfer)?;
                debit_receiver(state, transfer)?;
                credit_sender(state, transfer)?;
            }
            Transaction::Create(create) => {
                let sender_nonce = state.get_nonce(&create.from);
                let nonce = sender_nonce
                    .checked_sub(1)
                    .ok_or(CoreError::StateOverflow)?;
                let address = derive_contract_address(&create.from, nonce, chain_id)?;
                // S1.5-FBP04: the endowment only moved on a successful
                // constructor — mirror that here (missing receipt defaults
                // to success, like `unapply_call`). Failed deploys left no
                // account and no storage; the removes are no-ops then.
                let receipt = state
                    .receipts
                    .iter()
                    .find(|r| r.txid == txid(tx));
                let was_success = receipt
                    .map(|r| r.status == TxStatus::Success)
                    .unwrap_or(true);
                unapply_call_storage(state, txid(tx));
                if was_success {
                    // S1.5-FBP05: reverse the constructor's nested transfers
                    // first — they may touch the contract account, which is
                    // removed below. The endowment refund follows; removing
                    // the account reverts the up-front credit as a whole.
                    if let Some(transfers) = state.value_transfers_undo.remove(&txid(tx)) {
                        reverse_value_transfers(state, &transfers)?;
                    }
                    let receiver_balance = state.get_balance(&create.from);
                    let new_balance = receiver_balance
                        .checked_add(create.endowment)
                        .ok_or(CoreError::StateOverflow)?;
                    state.set_balance(&create.from, new_balance);
                } else {
                    // Failed deploys commit nothing; drop any stray record.
                    state.value_transfers_undo.remove(&txid(tx));
                }
                state.set_account(&address, Account::default());
                state
                    .contract_storage
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&address);
                state.set_nonce(&create.from, nonce);
            }
            Transaction::Call(call) => {
                let nonce = state
                    .get_nonce(&call.from)
                    .checked_sub(1)
                    .ok_or(CoreError::StateOverflow)?;
                state.set_nonce(&call.from, nonce);
                let receipt = state
                    .receipts
                    .iter()
                    .find(|r| r.txid == txid(tx));
                let reverse_value =
                    receipt.map(|r| r.status == TxStatus::Success).unwrap_or(true);
                if reverse_value {
                    // S1.5-FBP05: the undo record carries the full committed
                    // trail (top-level entry transfer + settled nested
                    // transfers), reversed in reverse commit order. A call
                    // without a record (transfer-only path written before
                    // the trail existed, defensive) falls back to reversing
                    // the top-level move reconstructed from the tx.
                    if let Some(transfers) = state.value_transfers_undo.remove(&txid(tx)) {
                        reverse_value_transfers(state, &transfers)?;
                    } else if call.value > 0 {
                        reverse_value_transfers(
                            state,
                            &[ValueTransfer {
                                from: call.from.clone(),
                                to: call.to.clone(),
                                amount: call.value,
                            }],
                        )?;
                    }
                }
                unapply_call_storage(state, txid(tx));
            }
        }
    }
    Ok(())
}

fn debit_receiver(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let receiver_balance = state.get_balance(&tx.to);
    let new_balance = receiver_balance
        .checked_sub(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.to, new_balance);
    Ok(())
}

fn credit_sender(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let sender_balance = state.get_balance(&tx.from);
    let new_balance = sender_balance
        .checked_add(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.from, new_balance);
    Ok(())
}

fn decrement_nonce(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let nonce = state.get_nonce(&tx.from);
    state.set_nonce(&tx.from, nonce - 1);
    Ok(())
}

/// Reverse the contract storage writes of one `Call` transaction
/// (S1.5-P15): restore every touched slot's pre-image from
/// `State::storage_undo`, then recompute `storage_root` for each affected
/// contract account. A contract whose storage becomes empty returns to the
/// `[0u8; 32]` sentinel. S1.5-FBP06: operates on the shared handle under
/// a single `write()` guard.
fn unapply_call_storage(state: &mut State, txid: [u8; 32]) {
    let Some(undo_entries) = state.storage_undo.remove(&txid) else {
        return;
    };
    let mut touched: HashSet<String> = HashSet::new();
    let mut storage_map = state
        .contract_storage
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (contract, key, preimage) in undo_entries {
        let storage = storage_map.entry(contract.clone()).or_default();
        match preimage {
            Some(value) => {
                storage.insert(key, value);
            }
            None => {
                storage.remove(&key);
            }
        }
        touched.insert(contract);
    }
    for contract in touched {
        let empty = storage_map
            .get(&contract)
            .is_some_and(HashMap::is_empty);
        if empty {
            storage_map.remove(&contract);
        }
        let root = storage_map
            .get(&contract)
            .map_or([0u8; 32], contract_storage_root);
        if let Some(Account::Contract(account)) = state.balances.get_mut(&contract) {
            account.storage_root = root;
        }
    }
    drop(storage_map);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::CHAIN_ID_REGTEST;

    /// All fixtures below are regtest chains (chain_id 3, reward 0).
    fn apply_regtest(state: &State, block: &Block) -> Result<State, CoreError> {
        apply_block(state, block, CHAIN_ID_REGTEST)
    }

    fn test_state_with_balance(addr: &str, balance: u64) -> State {
        let mut state = State::new();
        state.set_balance(addr, balance);
        state
    }

    fn coinbase_tx(receiver: &str, amount: u64) -> Transaction {
        Transaction::coinbase("coinbase", receiver, amount, 3)
    }

    fn transfer_tx(sender: &str, receiver: &str, amount: u64, nonce: u64) -> Transaction {
        Transaction::transfer(sender, receiver, amount, nonce, 3)
    }

    fn test_block(index: u64, transactions: Vec<Transaction>) -> Block {
        let txs = if index > 0 {
            let mut v = vec![coinbase_tx("miner", 0)];
            v.extend(transactions);
            v
        } else {
            transactions
        };
        Block {
            index,
            timestamp: 1000 + index * 600,
            transactions: txs,
            previous_hash: "prev_hash".to_string(),
            hash: format!("block_hash_{}", index),
            nonce: 0,
            target: "ff".to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        }
    }

    #[test]
    fn apply_genesis_creates_balances() {
        let state = State::new();
        let tx = coinbase_tx("alice", 1000);
        let block = test_block(0, vec![tx]);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 1000);
    }

    #[test]
    fn apply_genesis_multiple_recipients() {
        let state = State::new();
        let txs = vec![coinbase_tx("alice", 500), coinbase_tx("bob", 300)];
        let block = test_block(0, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 500);
        assert_eq!(new_state.get_balance("bob"), 300);
    }

    #[test]
    fn apply_block_deducts_sender() {
        let state = test_state_with_balance("alice", 1000);
        let txs = vec![transfer_tx("alice", "bob", 400, 1)];
        let block = test_block(1, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 600);
        assert_eq!(new_state.get_balance("bob"), 400);
    }

    #[test]
    fn apply_block_increments_nonce() {
        let mut state = test_state_with_balance("alice", 1000);
        state.set_nonce("alice", 0);
        let txs = vec![transfer_tx("alice", "bob", 100, 1)];
        let block = test_block(1, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_nonce("alice"), 1);
    }

    #[test]
    fn apply_block_rejects_insufficient_balance() {
        let state = test_state_with_balance("alice", 100);
        let txs = vec![transfer_tx("alice", "bob", 200, 1)];
        let block = test_block(1, txs);
        assert!(apply_regtest(&state, &block).is_err());
    }

    #[test]
    fn apply_block_coinbase_no_sender_check() {
        let state = State::new();
        let block = Block {
            index: 1,
            timestamp: 1600,
            transactions: vec![coinbase_tx("miner", 0)],
            previous_hash: "prev_hash".to_string(),
            hash: "block_hash_1".to_string(),
            nonce: 0,
            target: "ff".to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("miner"), 0);
    }

    #[test]
    fn unapply_genesis_removes_balances() {
        let state = test_state_with_balance("alice", 1000);
        let txs = vec![coinbase_tx("alice", 1000)];
        let block = test_block(0, txs);
        let applied = apply_regtest(&state, &block).unwrap();
        let restored = unapply_block(&applied, &block, CHAIN_ID_REGTEST).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn unapply_block_restores_nonce() {
        let mut state = test_state_with_balance("alice", 1000);
        state.set_nonce("alice", 0);
        let txs = vec![transfer_tx("alice", "bob", 100, 1)];
        let block = test_block(1, txs);
        let applied = apply_regtest(&state, &block).unwrap();
        assert_eq!(applied.get_nonce("alice"), 1);
        let restored = unapply_block(&applied, &block, CHAIN_ID_REGTEST).unwrap();
        assert_eq!(restored.get_nonce("alice"), 0);
        assert_eq!(restored.get_balance("alice"), 1000);
        assert_eq!(restored.get_balance("bob"), 0);
    }

    #[test]
    fn apply_unapply_roundtrip() {
        let mut state = State::new();
        state.set_balance("alice", 5000);
        state.set_balance("bob", 2000);
        state.set_nonce("alice", 0);
        state.set_nonce("bob", 0);

        let genesis_txs = vec![coinbase_tx("alice", 5000), coinbase_tx("bob", 2000)];
        let genesis = test_block(0, genesis_txs);
        let after_genesis = apply_regtest(&state, &genesis).unwrap();

        let txs = vec![
            transfer_tx("alice", "bob", 300, 1),
            transfer_tx("bob", "alice", 100, 1),
        ];
        let block1 = test_block(1, txs);
        let after_block1 = apply_regtest(&after_genesis, &block1).unwrap();

        let txs2 = vec![transfer_tx("alice", "bob", 500, 2)];
        let block2 = test_block(2, txs2);
        let after_block2 = apply_regtest(&after_block1, &block2).unwrap();

        let restored2 = unapply_block(&after_block2, &block2, CHAIN_ID_REGTEST).unwrap();
        assert_eq!(restored2, after_block1);

        let restored1 = unapply_block(&restored2, &block1, CHAIN_ID_REGTEST).unwrap();
        assert_eq!(restored1, after_genesis);

        let restored_genesis = unapply_block(&restored1, &genesis, CHAIN_ID_REGTEST).unwrap();
        assert_eq!(restored_genesis, state);
    }

    #[test]
    fn total_supply_tracks_correctly() {
        let state = State::new();
        let txs = vec![coinbase_tx("miner", 100)];
        let block = test_block(0, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.total_supply(), 100);
    }

    #[test]
    fn multiple_transactions_in_block() {
        let mut state = State::new();
        state.set_balance("alice", 10000);
        state.set_nonce("alice", 0);

        let txs = vec![
            transfer_tx("alice", "bob", 200, 1),
            transfer_tx("alice", "charlie", 300, 2),
        ];
        let block = test_block(1, txs);
        let new_state = apply_regtest(&state, &block).unwrap();

        assert_eq!(new_state.get_balance("alice"), 9500);
        assert_eq!(new_state.get_balance("bob"), 200);
        assert_eq!(new_state.get_balance("charlie"), 300);
        assert_eq!(new_state.get_nonce("alice"), 2);
    }

    #[test]
    fn apply_block_collects_transfer_receipts() {
        let state = test_state_with_balance("alice", 1000);
        let txs = vec![transfer_tx("alice", "bob", 400, 1)];
        let block = test_block(1, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.receipts.len(), 2);
        let transfer = &block.transactions[1];
        assert_eq!(new_state.receipts[1].txid, txid(transfer));
        assert_eq!(new_state.receipts[1].status, TxStatus::Success);
        assert_eq!(new_state.receipts[1].gas_used, 0);
        assert_eq!(new_state.receipts[1].block_height, 1);
        assert_eq!(new_state.receipts[1].tx_index, 1);
    }

    #[test]
    fn unapply_block_clears_receipts() {
        let state = test_state_with_balance("alice", 1000);
        let txs = vec![transfer_tx("alice", "bob", 400, 1)];
        let block = test_block(1, txs);
        let applied = apply_regtest(&state, &block).unwrap();
        assert!(!applied.receipts.is_empty());
        let restored = unapply_block(&applied, &block, CHAIN_ID_REGTEST).unwrap();
        assert!(restored.receipts.is_empty());
    }

    #[test]
    fn state_equality_ignores_receipts() {
        let mut a = test_state_with_balance("alice", 1000);
        let mut b = test_state_with_balance("alice", 1000);
        a.receipts = vec![Receipt {
            txid: [1u8; 32],
            gas_used: 10,
            logs: Vec::new(),
            status: TxStatus::Success,
            block_height: 1,
            tx_index: 0,
            value_transfers: Vec::new(),
        }];
        assert_eq!(a, b);
        b.set_balance("alice", 999);
        assert_ne!(a, b);
    }

    #[test]
    fn apply_block_without_vm_rejects_create_and_call() {
        let state = test_state_with_balance("alice", 1000);
        let create = Transaction::create("alice", vec![0x00], vec![], 10, 1, 3);
        let block = test_block(1, vec![create]);
        let err = apply_regtest(&state, &block).unwrap_err();
        assert!(matches!(err, CoreError::UnsupportedTxType { .. }));
    }

    // --- S1.5-FBP05: value-transfer commit/reverse helpers ---------------

    fn transfer(from: &str, to: &str, amount: u64) -> ValueTransfer {
        ValueTransfer {
            from: from.to_string(),
            to: to.to_string(),
            amount,
        }
    }

    #[test]
    fn commit_moves_balance_and_records_undo() {
        let mut state = test_state_with_balance("alice", 1000);
        let txid = [9u8; 32];
        let trail = vec![transfer("alice", "bob", 400)];
        commit_value_transfers(&mut state, txid, &trail).unwrap();
        assert_eq!(state.get_balance("alice"), 600);
        assert_eq!(state.get_balance("bob"), 400);
        assert_eq!(state.value_transfers_undo.get(&txid), Some(&trail));

        reverse_value_transfers(&mut state, &trail).unwrap();
        assert_eq!(state.get_balance("alice"), 1000);
        assert_eq!(state.get_balance("bob"), 0);
    }

    #[test]
    fn commit_self_transfer_is_a_no_op() {
        // Regression (S1.5-FBP05): the old read-both-then-write-both
        // pattern minted `amount` when `from == to`.
        let mut state = test_state_with_balance("alice", 1000);
        let trail = vec![transfer("alice", "alice", 500)];
        commit_value_transfers(&mut state, [1u8; 32], &trail).unwrap();
        assert_eq!(state.get_balance("alice"), 1000);

        reverse_value_transfers(&mut state, &trail).unwrap();
        assert_eq!(state.get_balance("alice"), 1000);
    }

    #[test]
    fn commit_rejects_insufficient_balance_atomically() {
        let mut state = test_state_with_balance("alice", 100);
        let err = commit_value_transfers(
            &mut state,
            [2u8; 32],
            &[transfer("alice", "bob", 101)],
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::StateOverflow));
        assert_eq!(state.get_balance("alice"), 100);
        assert_eq!(state.get_balance("bob"), 0);
        assert!(state.value_transfers_undo.is_empty());
    }

    #[test]
    fn chained_transfers_reverse_in_mirror_order() {
        let mut state = test_state_with_balance("a", 100);
        state.set_balance("b", 0);
        let txid = [3u8; 32];
        // a → b → c: b forwards what it just received.
        let trail = vec![transfer("a", "b", 50), transfer("b", "c", 50)];
        commit_value_transfers(&mut state, txid, &trail).unwrap();
        assert_eq!(state.get_balance("a"), 50);
        assert_eq!(state.get_balance("b"), 0);
        assert_eq!(state.get_balance("c"), 50);

        reverse_value_transfers(&mut state, &trail).unwrap();
        assert_eq!(state.get_balance("a"), 100);
        assert_eq!(state.get_balance("b"), 0);
        assert_eq!(state.get_balance("c"), 0);
    }
}
