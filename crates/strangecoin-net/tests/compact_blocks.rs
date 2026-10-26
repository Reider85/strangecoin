//! S2-P14 — BIP 152 compact blocks: fill-up, `GET_BLOCK_TXN`/`BLOCK_TXN`
//! round-trip, collision fallback and bandwidth reduction.
//!
//! All reconstruction tests run at the handler level (no sockets): a
//! broadcaster builds a [`CompactBlock`] and a receiver with a controlled
//! in-memory mempool ([`TestView`]) reconstructs it. The bandwidth test
//! measures the encoded wire size of a full block against its compact form.

use std::collections::HashMap;

use strangecoin_core::consensus::U256;
use strangecoin_core::serialize::{block_hash, merkle_root, txid};
use strangecoin_core::types::{
    AccountState, Block, ChainSnapshot, CreateTx, Transaction, TransferTx,
};
use strangecoin_net::gossip::compact_blocks::{
    apply_block_txn, fill_up, handle_get_block_txn, short_id, BlockTxn, CompactBlock, FillOutcome,
};
use strangecoin_net::protocol::encode_blocks;
use strangecoin_net::BlockchainView;

// ------------------------------------------------------------------- helpers

fn coinbase_tx() -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "rsc1miner".into(),
        to: "rsc1miner".into(),
        amount: 50,
        fee: 0,
        nonce: 0,
        chain_id: 3,
        signature: vec![2u8; 65],
        is_coinbase: true,
    })
}

fn transfer_tx(nonce: u64) -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "rsc1sender".into(),
        to: "rsc1receiver".into(),
        amount: 10,
        fee: 0,
        nonce,
        chain_id: 3,
        signature: vec![1u8; 65],
        is_coinbase: false,
    })
}

/// A create transaction whose `init_code` is padded to `code_len` bytes, used
/// to build a realistically large (~1 MB) block for the bandwidth test.
fn padded_create_tx(nonce: u64, code_len: usize) -> Transaction {
    Transaction::Create(CreateTx {
        from: "rsc1deployer".into(),
        init_code: vec![0x00u8; code_len],
        init_args: Vec::new(),
        constructor_params: Vec::new(),
        endowment: 0,
        gas_limit: 1_000_000,
        fee: 1,
        nonce,
        chain_id: 3,
        signature: vec![3u8; 65],
    })
}

fn make_block(txs: Vec<Transaction>) -> Block {
    let mut block = Block {
        index: 9,
        timestamp: 1_700_000_009,
        transactions: txs,
        previous_hash: "ee".repeat(32),
        hash: String::new(),
        nonce: 44,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [7u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    let ids: Vec<[u8; 32]> = block.transactions.iter().map(txid).collect();
    block.tx_root = merkle_root(&ids);
    block.hash = hex::encode(block_hash(&block));
    block
}

/// In-memory `BlockchainView` with a controllable mempool + a one-block chain.
#[derive(Default)]
struct TestView {
    mempool: HashMap<[u8; 32], Transaction>,
    blocks: HashMap<String, Block>,
}

impl TestView {
    fn with_mempool(txs: Vec<Transaction>) -> Self {
        let mut mempool = HashMap::new();
        for tx in txs {
            mempool.insert(txid(&tx), tx);
        }
        TestView {
            mempool,
            blocks: HashMap::new(),
        }
    }
}

impl BlockchainView for TestView {
    fn tip_hash(&self) -> String {
        self.blocks
            .values()
            .max_by_key(|b| b.index)
            .map(|b| b.hash.clone())
            .unwrap_or_default()
    }

    fn chain_len(&self) -> usize {
        self.blocks.len()
    }

    fn adopt_candidate(
        &self,
        _candidate_chain: Vec<Block>,
        _candidate_balances: Option<HashMap<String, AccountState>>,
        _candidate_mempool_txs: Vec<Transaction>,
        _candidate_difficulty: u32,
    ) -> Result<bool, String> {
        Ok(false)
    }

    fn apply_tx(&self, _transaction: Transaction) -> Result<(), String> {
        Ok(())
    }

    fn add_block(&self, _block: Block) -> Result<(), String> {
        Ok(())
    }

    fn chain_contains_tx(&self, _tx: &Transaction) -> bool {
        false
    }

    fn mempool_contains(&self, txid: &[u8; 32]) -> bool {
        self.mempool.contains_key(txid)
    }

    fn mempool_tx(&self, txid: &[u8; 32]) -> Option<Transaction> {
        self.mempool.get(txid).cloned()
    }

    fn mempool_txs(&self) -> Vec<Transaction> {
        self.mempool.values().cloned().collect()
    }

    fn block_by_hash(&self, block_hash: &str) -> Option<Block> {
        self.blocks.get(block_hash).cloned()
    }

    fn save_state(&self) {}

    fn chain_contains_block(&self, block_hash: &str) -> bool {
        self.blocks.contains_key(block_hash)
    }

    fn snapshot_wire(&self) -> ChainSnapshot {
        ChainSnapshot {
            chain: Vec::new(),
            balances: HashMap::new(),
            difficulty: 0,
            mempool_txs: self.mempool.values().cloned().collect(),
            total_work: U256::default(),
        }
    }
}

// --------------------------------------------------------------------- tests

/// 100-tx block, every tx (and the coinbase) available: the receiver
/// reconstructs the whole block from the mempool with no `GET_BLOCK_TXN`.
#[test]
fn fill_up_completes_from_mempool_without_roundtrip() {
    let mut body = Vec::with_capacity(100);
    for i in 0..100u64 {
        body.push(transfer_tx(i));
    }
    let mut txs = vec![coinbase_tx()];
    txs.extend(body.iter().cloned());
    let block = make_block(txs);

    let compact = CompactBlock::from_block(&block);
    // coinbase is prefilled; all 100 body txs carry short IDs.
    assert_eq!(compact.prefilled_txs.len(), 1);
    assert_eq!(compact.short_ids.len(), 101);

    let view = TestView::with_mempool(body);
    match fill_up(&compact, &view).unwrap() {
        FillOutcome::Complete(rebuilt) => {
            assert_eq!(rebuilt.hash, block.hash);
            assert_eq!(rebuilt.tx_root, block.tx_root);
            assert_eq!(rebuilt.transactions.len(), block.transactions.len());
            for (a, b) in rebuilt.transactions.iter().zip(&block.transactions) {
                assert_eq!(txid(a), txid(b));
            }
        }
        FillOutcome::Missing(req) => panic!("expected full reconstruction, got {req:?}"),
    }
}

/// 100-tx block with 20 txs missing from the mempool: `fill_up` returns a
/// `GetBlockTxn`; the round-trip (`handle_get_block_txn` -> `apply_block_txn`)
/// completes the block.
#[test]
fn missing_txs_roundtrip_completes_block() {
    let mut all_body: Vec<Transaction> = Vec::with_capacity(100);
    for i in 0..100u64 {
        all_body.push(transfer_tx(i));
    }
    // The receiver holds every tx except indices 40..60 of the body.
    let held: Vec<Transaction> = all_body
        .iter()
        .enumerate()
        .filter(|(i, _)| !(40..60).contains(i))
        .map(|(_, tx)| tx.clone())
        .collect();
    assert_eq!(held.len(), 80);

    let mut txs = vec![coinbase_tx()];
    txs.extend(all_body.iter().cloned());
    let block = make_block(txs);

    let compact = CompactBlock::from_block(&block);
    let receiver = TestView::with_mempool(held);

    let req = match fill_up(&compact, &receiver).unwrap() {
        FillOutcome::Missing(req) => req,
        FillOutcome::Complete(_) => panic!("expected missing txs"),
    };
    // 20 missing body indices (coinbase is prefilled).
    assert_eq!(req.missing_indices.len(), 20);

    // Broadcaster side: serve the request from the full block.
    let mut broadcaster = TestView::default();
    broadcaster.blocks.insert(block.hash.clone(), block.clone());
    let txn: BlockTxn = handle_get_block_txn(&req, &broadcaster).expect("block is known");
    assert_eq!(txn.txs.len(), 20);

    let rebuilt =
        apply_block_txn(&compact, &req.missing_indices, &txn, &receiver).expect("apply succeeds");
    assert_eq!(rebuilt.hash, block.hash);
    assert_eq!(rebuilt.transactions.len(), 101);
}

/// `handle_get_block_txn` returns `None` for an unknown block or an
/// out-of-range index (caller falls back to a full-block request).
#[test]
fn handle_get_block_txn_rejects_unknown_block_and_bad_index() {
    let block = make_block(vec![coinbase_tx(), transfer_tx(1)]);
    let view = TestView::default(); // empty chain

    let req = strangecoin_net::GetBlockTxn {
        block_hash: [9u8; 32],
        missing_indices: vec![0],
    };
    assert!(handle_get_block_txn(&req, &view).is_none());

    let mut with_block = TestView::default();
    with_block.blocks.insert(block.hash.clone(), block.clone());
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&block_hash(&block));
    let bad_index = strangecoin_net::GetBlockTxn {
        block_hash: hash,
        missing_indices: vec![0, 99], // 99 out of range
    };
    assert!(handle_get_block_txn(&bad_index, &with_block).is_none());
}

/// Short-ID collision: a tampered compact block whose committed `tx_root`
/// cannot be satisfied is rejected — never reconstructed into a false block.
///
/// A true siphash collision is not constructible on demand, so this test
/// simulates the failure mode a collision would produce: the receiver's mempool
/// yields a transaction whose short ID matches a slot but whose txid does not
/// belong to the committed set. `apply_block_txn`'s merkle check catches it.
#[test]
fn short_id_collision_or_tamper_falls_back_not_false_block() {
    let real_txs = vec![coinbase_tx(), transfer_tx(1), transfer_tx(2)];
    let block = make_block(real_txs.clone());
    let compact = CompactBlock::from_block(&block);

    // Broadcaster has the block; the request asks for index 1.
    let mut broadcaster = TestView::default();
    broadcaster.blocks.insert(block.hash.clone(), block.clone());
    let hash = {
        let mut h = [0u8; 32];
        h.copy_from_slice(&block_hash(&block));
        h
    };
    let req = strangecoin_net::GetBlockTxn {
        block_hash: hash,
        missing_indices: vec![1],
    };
    let good_txn = handle_get_block_txn(&req, &broadcaster).unwrap();

    // The receiver still has the other body txs in its mempool.
    let receiver = TestView::with_mempool(vec![transfer_tx(2)]);

    // Tamper: swap the body for a different tx that is NOT at that index.
    let tampered = BlockTxn {
        block_hash: hash,
        txs: vec![transfer_tx(99)],
    };
    assert!(
        apply_block_txn(&compact, &req.missing_indices, &tampered, &receiver).is_err()
    );

    // A short-ID mismatch is also caught before the merkle check.
    // Force a short-id mismatch by corrupting the compact block's committed id.
    let mut bad_compact = compact.clone();
    bad_compact.short_ids[1] = [0xAB; 8];
    assert!(
        apply_block_txn(&bad_compact, &req.missing_indices, &good_txn, &receiver).is_err()
    );

    // The honest path still succeeds (sanity check for the negative cases above).
    assert!(apply_block_txn(&compact, &req.missing_indices, &good_txn, &receiver).is_ok());
}

/// Bandwidth reduction: a ~1 MB full block relays as a ~1.5 KB compact block
/// (header + 8 bytes/tx). Must be >= 50% reduction (ROADMAP3 DoD).
#[test]
fn bandwidth_reduction_at_least_50_percent() {
    // 100 create txs x ~10 KB init_code ~= 1 MB of transaction data.
    let mut txs = vec![coinbase_tx()];
    for i in 0..100u64 {
        txs.push(padded_create_tx(i, 10 * 1024));
    }
    let block = make_block(txs);

    let full_wire = encode_blocks(std::slice::from_ref(&block)).unwrap();
    let compact_wire = CompactBlock::from_block(&block).encode().unwrap();

    let full_len = full_wire.len();
    let compact_len = compact_wire.len();
    // Sanity: the full block really is ~1 MB and the compact form ~1.5 KB.
    assert!(
        full_len >= 1_000_000,
        "full block only {full_len} bytes, expected ~1 MB"
    );
    assert!(
        compact_len < 4096,
        "compact block unexpectedly large: {compact_len} bytes"
    );
    // >= 50% reduction (in practice ~99.8%).
    let reduction = 1.0 - (compact_len as f64 / full_len as f64);
    assert!(
        reduction >= 0.5,
        "bandwidth reduction only {:.2}% ({compact_len} vs {full_len})",
        reduction * 100.0
    );
}

/// `short_id` is stable for a fixed nonce and flips when the nonce changes.
#[test]
fn short_id_stable_and_nonce_dependent() {
    let id = [7u8; 32];
    assert_eq!(short_id(&id, 1), short_id(&id, 1));
    assert_ne!(short_id(&id, 1), short_id(&id, 2));
}
