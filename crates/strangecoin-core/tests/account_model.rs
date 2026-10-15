//! S1.5-P07: Account model — EOA + contract accounts (code_hash, storage_root).

use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::sparse_merkle::SparseMerkleTrie;
use strangecoin_core::state::{apply_block, unapply_block, State};
use strangecoin_core::types::{Account, Block, ContractAccount, Transaction};

const CODE_HASH: [u8; 32] = [7u8; 32];
const STORAGE_ROOT: [u8; 32] = [9u8; 32];

fn coinbase_tx(receiver: &str, amount: u64) -> Transaction {
    Transaction {
        sender: "coinbase".to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce: 0,
        chain_id: CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: true,
    }
}

fn transfer_tx(sender: &str, receiver: &str, amount: u64, nonce: u64) -> Transaction {
    Transaction {
        sender: sender.to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce,
        chain_id: CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: false,
    }
}

fn regtest_block(index: u64, transactions: Vec<Transaction>) -> Block {
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
        previous_hash: "prev".to_string(),
        hash: format!("block_{index}"),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

#[test]
fn missing_account_defaults_to_empty_eoa() {
    let state = State::new();
    assert_eq!(state.get_account("nobody"), None);
    assert_eq!(state.get_balance("nobody"), 0);
    assert_eq!(state.get_nonce("nobody"), 0);
    assert!(!state.balances.contains_key("nobody"));
}

#[test]
fn eoa_create_and_read() {
    let mut state = State::new();
    state.set_account("alice", Account::eoa(1000, 3));
    let account = state.get_account("alice").unwrap();
    assert!(!account.is_contract());
    assert_eq!(account.balance(), 1000);
    assert_eq!(account.nonce(), 3);
}

#[test]
fn contract_create_via_set_account() {
    let mut state = State::new();
    state.set_account("contract", Account::contract(CODE_HASH, STORAGE_ROOT));
    let account = state.get_account("contract").unwrap();
    let contract = account.as_contract().unwrap();
    assert!(account.is_contract());
    assert_eq!(contract.code_hash, CODE_HASH);
    assert_eq!(contract.storage_root, STORAGE_ROOT);
    assert_eq!(contract.balance, 0);
    assert_eq!(contract.nonce, 0);
}

#[test]
fn contract_appears_in_state_root() {
    let mut with_contract = State::new();
    with_contract.set_account("contract", Account::contract(CODE_HASH, STORAGE_ROOT));
    let with_root = SparseMerkleTrie::compute_root(&with_contract.balances);

    let without = State::new();
    let without_root = SparseMerkleTrie::compute_root(&without.balances);
    assert_ne!(with_root, without_root);
    assert_ne!(with_root, SparseMerkleTrie::empty_root());
}

/// S1.5-P07 / precondition for S1.5-P15: a storage_root update changes the
/// global state root.
#[test]
fn contract_storage_update_changes_state_root() {
    let mut state = State::new();
    state.set_account("contract", Account::contract(CODE_HASH, STORAGE_ROOT));
    let root_before = SparseMerkleTrie::compute_root(&state.balances);

    state.set_account(
        "contract",
        Account::Contract(ContractAccount {
            code_hash: CODE_HASH,
            storage_root: [8u8; 32],
            nonce: 0,
            balance: 0,
        }),
    );
    let root_after = SparseMerkleTrie::compute_root(&state.balances);
    assert_ne!(root_before, root_after);
}

/// A contract with zero balance/nonce is never pruned — code_hash pins it.
#[test]
fn contract_not_pruned_on_zero_balance() {
    let mut state = State::new();
    state.set_account("contract", Account::contract(CODE_HASH, STORAGE_ROOT));
    state.set_balance("contract", 0);
    state.set_nonce("contract", 0);
    assert!(state.get_account("contract").is_some());

    let root = SparseMerkleTrie::compute_root(&state.balances);
    let empty = State::new();
    assert_ne!(root, SparseMerkleTrie::compute_root(&empty.balances));
}

/// A phantom contract (all-zero record) is treated as absent.
#[test]
fn phantom_contract_is_empty() {
    let phantom = Account::Contract(ContractAccount::default());
    assert!(phantom.is_empty());
    let mut state = State::new();
    state.set_account("phantom", phantom);
    assert!(state.get_account("phantom").is_none());
}

/// Crediting a contract must not turn it into an EOA (variant preservation).
#[test]
fn apply_block_preserves_contract_variant() {
    let mut state = State::new();
    state.set_account("alice", Account::eoa(1000, 0));
    state.set_account("contract", Account::contract(CODE_HASH, STORAGE_ROOT));

    let block = regtest_block(1, vec![transfer_tx("alice", "contract", 500, 1)]);
    let applied = apply_block(&state, &block, CHAIN_ID_REGTEST).unwrap();

    let contract = applied.get_account("contract").unwrap();
    assert!(contract.is_contract());
    let contract = contract.as_contract().unwrap();
    assert_eq!(contract.balance, 500);
    assert_eq!(contract.code_hash, CODE_HASH);
    assert_eq!(contract.storage_root, STORAGE_ROOT);
    assert_eq!(applied.get_balance("alice"), 500);
    assert_eq!(applied.get_nonce("alice"), 1);
}

/// apply → unapply round-trips a chain that touches a contract account.
#[test]
fn apply_unapply_roundtrip_with_contract() {
    let mut state = State::new();
    state.set_account("alice", Account::eoa(1000, 0));
    state.set_account("contract", Account::contract(CODE_HASH, STORAGE_ROOT));

    let genesis = regtest_block(0, vec![coinbase_tx("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis, CHAIN_ID_REGTEST).unwrap();

    let block = regtest_block(1, vec![transfer_tx("alice", "contract", 250, 1)]);
    let applied = apply_block(&after_genesis, &block, CHAIN_ID_REGTEST).unwrap();

    let restored = unapply_block(&applied, &block).unwrap();
    assert_eq!(restored, after_genesis);
    let contract = restored.get_account("contract").unwrap().as_contract().unwrap();
    assert_eq!(contract.balance, 0);
    assert_eq!(contract.code_hash, CODE_HASH);
}

// ---------------------------------------------------------------------------
// Serde compatibility (LevelDB persistence / wire snapshots)
// ---------------------------------------------------------------------------

#[test]
fn legacy_flat_json_deserializes_as_eoa() {
    let json = r#"{"balance":42,"nonce":7}"#;
    let account: Account = serde_json::from_str(json).unwrap();
    assert_eq!(account, Account::eoa(42, 7));
    assert!(!account.is_contract());
}

#[test]
fn eoa_serializes_to_legacy_flat_json() {
    let account = Account::eoa(42, 7);
    let json = serde_json::to_string(&account).unwrap();
    assert_eq!(json, r#"{"balance":42,"nonce":7}"#);
}

#[test]
fn contract_serde_roundtrip() {
    let account = Account::Contract(ContractAccount {
        code_hash: CODE_HASH,
        storage_root: STORAGE_ROOT,
        nonce: 5,
        balance: 100,
    });
    let json = serde_json::to_string(&account).unwrap();
    let back: Account = serde_json::from_str(&json).unwrap();
    assert_eq!(back, account);
    let contract = back.as_contract().unwrap();
    assert_eq!(contract.code_hash, CODE_HASH);
    assert_eq!(contract.storage_root, STORAGE_ROOT);
}

#[test]
fn storage_root_without_code_hash_rejected() {
    let json = r#"{"balance":1,"nonce":0,"storage_root":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}"#;
    assert!(serde_json::from_str::<Account>(json).is_err());
}

/// Contract with missing storage_root defaults it to zero (tolerant load).
#[test]
fn contract_without_storage_root_defaults_zero() {
    let code_hash_json = serde_json::to_string(&CODE_HASH).unwrap();
    let json = format!(r#"{{"balance":0,"nonce":0,"code_hash":{code_hash_json}}}"#);
    let account: Account = serde_json::from_str(&json).unwrap();
    let contract = account.as_contract().unwrap();
    assert_eq!(contract.code_hash, CODE_HASH);
    assert_eq!(contract.storage_root, [0u8; 32]);
}

/// A zero-balance contract is NOT pruned from the SMT (compute_root filter).
#[test]
fn compute_root_keeps_zero_balance_contract() {
    let mut accounts = std::collections::HashMap::new();
    accounts.insert(
        "contract".to_string(),
        Account::contract(CODE_HASH, STORAGE_ROOT),
    );
    let root = SparseMerkleTrie::compute_root(&accounts);
    assert_ne!(root, SparseMerkleTrie::empty_root());
}
