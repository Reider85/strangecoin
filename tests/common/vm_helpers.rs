//! Shared helpers for the Stage 1.5 VM integration tests (S1.5-P17).
//!
//! Two execution levels are covered:
//!
//! * **component level** — signed transactions through
//!   [`validate_and_apply`](strangecoin::blockchain::block_executor::validate_and_apply):
//!   signatures, chain id, block gas limit, VM execution, receipts and the
//!   state-root commitment all run for real;
//! * **core level** — unsigned transactions through
//!   `apply_block_with_vm` with a hand-wired [`WasmiExecutor`] (needed when
//!   the test must register contract ABIs, which the node path does not
//!   propagate yet).
//!
//! All scenarios run on regtest (`chain_id = 3`); no temp dirs, no sleeps.

use std::collections::HashMap;

use secp256k1::SecretKey;
use strangecoin::blockchain::block_executor::{now_secs, validate_and_apply, BlockView};
use strangecoin::consensus::CURRENT_CONSENSUS_VERSION;
use strangecoin::test_support::{generate_keypair, sign_transaction};
use strangecoin::{Block, Transaction};
use strangecoin_core::address::derive_contract_address;
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::{apply_block_with_vm, compute_state_root, State};
use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{BlockContext, ExecutionContext};
use strangecoin_vm_wasm::WasmiExecutor;

/// Maximal regtest target — inherited by every child until the first
/// retarget, so sealed blocks pass proof of work as-is.
pub const TARGET_MAX: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

// ---------------------------------------------------------------------------
// WAT construction (same pattern as the `strangecoin-vm-wasm` test suite)
// ---------------------------------------------------------------------------

pub fn hex_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// Build a WAT module: hoisted imports, one page of memory, data segments,
/// then the caller's body (which must export `main`).
pub fn wat(segments: &[(u32, &[u8])], body: &str) -> String {
    let mut imports = String::new();
    let mut rest = String::new();
    for line in body.lines() {
        if line.trim_start().starts_with("(import ") {
            imports.push_str(line.trim_start());
        } else {
            rest.push_str(line);
        }
        rest.push('\n');
    }
    let mut out = String::from("(module ");
    out.push_str(&imports);
    out.push_str("(memory (export \"memory\") 1)");
    for (offset, bytes) in segments {
        out.push_str(&format!(
            "(data (i32.const {offset}) \"{}\")",
            hex_escape(bytes)
        ));
    }
    out.push_str(&rest);
    out.push(')');
    out
}

// ---------------------------------------------------------------------------
// Keys and signing
// ---------------------------------------------------------------------------

pub type Keypair = (String, SecretKey);

pub fn keypair() -> Keypair {
    generate_keypair()
}

/// Sign `tx` with `sk` (blake3 digest of the canonical unsigned encoding)
/// and return it ready for `validate_and_apply`.
pub fn sign(mut tx: Transaction, sk: &SecretKey) -> Transaction {
    sign_transaction(&mut tx, sk);
    tx
}

// ---------------------------------------------------------------------------
// Block construction — component level
// ---------------------------------------------------------------------------

pub fn coinbase_tx() -> Transaction {
    Transaction::coinbase("coinbase", "rsc1miner", 0, CHAIN_ID_REGTEST)
}

pub fn seal(mut block: Block) -> Block {
    block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    block
}

/// Genesis granting `funded` `amount` (coinbase-style, exempt from
/// signature verification) under the maximal regtest target.
pub fn genesis_grant(funded: &str, amount: u64) -> Block {
    seal(Block {
        index: 0,
        timestamp: 0,
        transactions: vec![Transaction::coinbase(
            "coinbase",
            funded,
            amount,
            CHAIN_ID_REGTEST,
        )],
        previous_hash: "0".repeat(64),
        hash: String::new(),
        nonce: 0,
        target: TARGET_MAX.to_string(),
        consensus_version: CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    })
}

/// Start a component-level chain: genesis granting `funded` `amount`, plus
/// the genesis post-state. `tx_root`/header hash are sealed; the coinbase is
/// prepended by [`child_block`], never here (genesis carries its own).
pub fn init_chain(funded: &str, amount: u64) -> (Vec<Block>, State) {
    let genesis = genesis_grant(funded, amount);
    let empty_chain: Vec<Block> = Vec::new();
    let view = BlockView::new(
        &empty_chain,
        now_secs(),
        false,
        CURRENT_CONSENSUS_VERSION,
        CHAIN_ID_REGTEST,
    );
    let state =
        validate_and_apply(&State::new(), &genesis, &view).expect("genesis must apply");
    (vec![genesis], state)
}

/// Unsigned child of `parent` with a prepended zero-amount coinbase.
/// Timestamp is strictly increasing (`now + index`) and within the future
/// bound; `state_root` stays zero until [`commit_block`] precomputes it.
pub fn child_block(parent: &Block, mut transactions: Vec<Transaction>) -> Block {
    transactions.insert(0, coinbase_tx());
    let index = parent.index + 1;
    Block {
        index,
        timestamp: now_secs() + index,
        transactions,
        previous_hash: parent.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: parent.target.clone(),
        consensus_version: CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

/// Executor seeded with the chain's contract codes.
pub fn executor_with(codes: &HashMap<String, Vec<u8>>) -> WasmiExecutor {
    let mut vm = WasmiExecutor::new();
    for (address, wasm) in codes {
        vm.insert_code(address, wasm.clone());
    }
    vm
}

/// Commit `block` on top of `(chain, state)`: precompute the VM-aware
/// post-state root into the header (invariant #19), seal the header hash,
/// then run the full `validate_and_apply` pipeline. Returns the post state
/// and appends the block to `chain`.
pub fn commit_block(
    chain: &mut Vec<Block>,
    state: &State,
    mut block: Block,
    codes: &HashMap<String, Vec<u8>>,
) -> State {
    let mut vm = executor_with(codes);
    let post = apply_block_with_vm(state, &block, CHAIN_ID_REGTEST, &mut vm)
        .expect("block must apply at the state level");
    block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
    block.state_root = compute_state_root(&post.balances);
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));

    let view = BlockView::new(
        chain,
        now_secs(),
        false,
        CURRENT_CONSENSUS_VERSION,
        CHAIN_ID_REGTEST,
    )
    .with_codes(codes);
    let applied = validate_and_apply(state, &block, &view)
        .expect("crafted block must pass full validation");
    chain.push(block);
    applied
}

/// Contract address derived from `(creator, state_nonce)` — `apply_create`
/// uses the creator's *current state nonce*, not the tx's nonce field.
pub fn derive(creator: &str, state_nonce: u64) -> String {
    derive_contract_address(creator, state_nonce, CHAIN_ID_REGTEST)
        .expect("derive contract address")
}

// ---------------------------------------------------------------------------
// Block construction — core level (no header validation)
// ---------------------------------------------------------------------------

/// Core-level block: prepended zero-coinbase + `transactions`; header fields
/// are placeholders (only `apply_block_with_vm` consumes this).
pub fn core_block(index: u64, transactions: Vec<Transaction>) -> Block {
    let mut all = vec![coinbase_tx()];
    all.extend(transactions);
    Block {
        index,
        timestamp: 1000 + index * 600,
        transactions: all,
        previous_hash: "prev_hash".to_string(),
        hash: format!("block_hash_{index}"),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

/// Apply a core-level block with a freshly seeded executor.
pub fn core_apply(
    state: &State,
    block: &Block,
    codes: &HashMap<String, Vec<u8>>,
) -> State {
    let mut vm = executor_with(codes);
    apply_block_with_vm(state, block, CHAIN_ID_REGTEST, &mut vm)
        .expect("core-level block must apply")
}

/// Minimal [`ExecutionContext`] for direct executor probes (pinning return
/// values and exact [`VmError`](strangecoin_core::vm::VmError) variants).
pub fn execution_context(callee: &str, input: Vec<u8>, gas_limit: u64) -> ExecutionContext {
    let mut accounts = HashMap::new();
    accounts.insert(callee.to_string(), AccountState::default());
    ExecutionContext {
        block: BlockHeader {
            index: 1,
            timestamp: 1_700_000_000,
            previous_hash: "0".repeat(64),
            hash: "0".repeat(64),
            nonce: 0,
            target: "ffff".to_string(),
            consensus_version: CURRENT_CONSENSUS_VERSION,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        },
        accounts,
        caller: "rsc1probe".to_string(),
        callee: callee.to_string(),
        value: 0,
        gas_limit,
        gas_used: 0,
        depth: 0,
        reentrant: false,
        read_only: false,
        block_context: BlockContext {
            height: 1,
            timestamp: 1_700_000_000,
            coinbase: "rsc1miner".to_string(),
            block_gas_limit: strangecoin_core::vm::BLOCK_GAS_LIMIT,
        },
        storage: HashMap::new(),
        storage_overlay: HashMap::new(),
        storage_address: callee.to_string(),
        input,
    }
}
