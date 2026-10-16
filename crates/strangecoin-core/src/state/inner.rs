use std::collections::HashMap;

use crate::address::derive_contract_address;
use crate::economics::emission::block_reward_at_height_for_chain;
use crate::error::CoreError;
use crate::serialize::txid;
use crate::types::{Account, Block, ContractAccount, Transaction, TransferTx};
use crate::vm::{
    BlockContext, ExecutionContext, ExecutionResult, Opcode, Receipt, TxStatus, VmError, VmExecutor,
    BLOCK_GAS_LIMIT,
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

/// Post-block chain state: account balances plus the receipts of the most
/// recently applied block (S1.5-P14, invariant #18).
///
/// `receipts` is derived data — a pure function of the applied block — and is
/// replaced wholesale on every apply and cleared on unapply. `PartialEq` and
/// `Eq` deliberately compare **balances only**: the apply/unapply round-trip
/// (invariant #4, on which reorg correctness rests) is defined over the
/// account state; receipts of any height are recomputed from the chain.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub balances: HashMap<String, Account>,
    pub receipts: Vec<Receipt>,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.balances == other.balances
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
    let mut receipts: Vec<Receipt> = Vec::with_capacity(block.transactions.len());

    if block.index == 0 {
        apply_genesis(&mut new_state, block, chain_id, &mut receipts)?;
    } else {
        apply_non_genesis(&mut new_state, block, chain_id, vm, &mut receipts)?;
    }

    new_state.receipts = receipts;
    Ok(new_state)
}

fn transfer_receipt(tx: &Transaction, block: &Block, tx_index: u32) -> Receipt {
    Receipt {
        txid: txid(tx),
        gas_used: 0,
        logs: Vec::new(),
        status: TxStatus::Success,
        block_height: block.index,
        tx_index,
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
                receipts.push(transfer_receipt(tx, block, tx_index as u32));
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
                receipts.push(transfer_receipt(tx, block, tx_index as u32));
            }
            Transaction::Create(create) => match vm.as_deref_mut() {
                None => {
                    return Err(CoreError::UnsupportedTxType {
                        reason: "create transactions require a VM executor (S1.5-P14)"
                            .to_string(),
                    })
                }
                Some(vm) => {
                    let receipt =
                        apply_create(state, create, chain_id, vm, block.index, tx_index as u32)?;
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

/// Deploy path (S1.5-P14): install the contract account and register its
/// code with the executor. The constructor export is not executed (MVP —
/// S1.5-P17 exercises the deployed entry point directly); `init_args` are
/// carried in the signed tx for forward compatibility.
fn apply_create<V: VmExecutor>(
    state: &mut State,
    create: &crate::types::CreateTx,
    chain_id: u32,
    vm: &mut V,
    block_height: u64,
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
    state.set_balance(&create.from, sender_balance - create.endowment);

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

    Ok(Receipt {
        txid: txid(&Transaction::Create(create.clone())),
        gas_used: 0,
        logs: Vec::new(),
        status: TxStatus::Success,
        block_height,
        tx_index,
    })
}

/// Call path (S1.5-P14): execute the contract entry point through `vm`.
/// The value transfer is applied only on `Success`; a failed frame (revert,
/// out-of-gas, engine error) still increments the sender nonce and produces
/// a receipt. A call to an EOA (no contract account) is a plain value
/// transfer with a zero-gas success receipt.
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

    let is_contract = state
        .get_account(&call.to)
        .is_some_and(|account| account.is_contract());
    if !is_contract {
        if call.value > 0 {
            let new_sender = state
                .get_balance(&call.from)
                .checked_sub(call.value)
                .ok_or(CoreError::StateOverflow)?;
            let new_receiver = state
                .get_balance(&call.to)
                .checked_add(call.value)
                .ok_or(CoreError::StateOverflow)?;
            state.set_balance(&call.from, new_sender);
            state.set_balance(&call.to, new_receiver);
        }
        return Ok(Receipt {
            txid: txid(&Transaction::Call(call.clone())),
            gas_used: 0,
            logs: Vec::new(),
            status: TxStatus::Success,
            block_height: block.index,
            tx_index,
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
        storage_overlay: HashMap::new(),
        input: call.data.clone(),
    };

    let call_tx = Transaction::Call(call.clone());
    match vm.execute(ctx) {
        Ok(result) => {
            if result.status == TxStatus::Success && call.value > 0 {
                let new_sender = state
                    .get_balance(&call.from)
                    .checked_sub(call.value)
                    .ok_or(CoreError::StateOverflow)?;
                let new_receiver = state
                    .get_balance(&call.to)
                    .checked_add(call.value)
                    .ok_or(CoreError::StateOverflow)?;
                state.set_balance(&call.from, new_sender);
                state.set_balance(&call.to, new_receiver);
            }
            Ok(Receipt::from_execution(
                txid(&call_tx),
                block.index,
                tx_index,
                &result,
            ))
        }
        Err(err) => {
            let vm_error: VmError = err.into();
            Ok(Receipt::from_vm_error(
                txid(&call_tx),
                block.index,
                tx_index,
                &vm_error,
                call.gas_limit,
            ))
        }
    }
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
                state.set_account(&address, Account::default());
                let receiver_balance = state.get_balance(&create.from);
                let new_balance = receiver_balance
                    .checked_add(create.endowment)
                    .ok_or(CoreError::StateOverflow)?;
                state.set_balance(&create.from, new_balance);
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
                if reverse_value && call.value > 0 {
                    let callee_balance = state.get_balance(&call.to);
                    let new_callee = callee_balance
                        .checked_sub(call.value)
                        .ok_or(CoreError::StateOverflow)?;
                    let sender_balance = state.get_balance(&call.from);
                    let new_sender = sender_balance
                        .checked_add(call.value)
                        .ok_or(CoreError::StateOverflow)?;
                    state.set_balance(&call.to, new_callee);
                    state.set_balance(&call.from, new_sender);
                }
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
}
