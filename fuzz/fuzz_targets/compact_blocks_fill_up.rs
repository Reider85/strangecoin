//! S2-P21 — BIP 152 compact-block fill-up fuzzing (S2-P14).
//!
//! Contract: a random block turned into a `CompactBlock`, filled up against
//! a random mempool, and (on the corruption path) wire-flipped before
//! re-parsing, never panics. Whenever fill-up reports `Complete`, the
//! rebuilt block's `tx_root` matches the transactions it actually carries —
//! a short-ID collision can force `Missing`/an error, never a false block.
//! Whenever it reports `Missing`, the requested indices are in range and
//! the `GET_BLOCK_TXN` → `BLOCK_TXN` round-trip completes the block.

#![no_main]

use std::collections::{HashMap, HashSet};

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use strangecoin_core::consensus::U256;
use strangecoin_core::serialize::{block_hash, merkle_root, txid};
use strangecoin_core::types::{AccountState, Block, ChainSnapshot, Transaction, TransferTx};
use strangecoin_net::gossip::compact_blocks::{
    apply_block_txn, fill_up, handle_get_block_txn, CompactBlock, FillOutcome,
};
use strangecoin_net::gossip::GossipMessage;
use strangecoin_net::BlockchainView;

const MAX_BODY_TXS: usize = 32;
const MAX_CORRUPTIONS: usize = 16;

#[derive(Arbitrary, Debug)]
struct CompactInput {
    /// Nonces of the block's body transactions.
    nonces: Vec<u64>,
    /// Parallel to the deduplicated body: `true` = the receiver lacks it.
    missing: Vec<bool>,
    /// Values mixed into wire-byte flips on the encoded compact block.
    corruptions: Vec<u8>,
}

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

/// In-memory `BlockchainView`: controllable mempool + an optional chain of
/// full blocks (the broadcaster side of the round-trip).
#[derive(Default)]
struct MockView {
    mempool: HashMap<[u8; 32], Transaction>,
    blocks: HashMap<String, Block>,
}

impl MockView {
    fn with_txs(txs: Vec<Transaction>) -> Self {
        let mut mempool = HashMap::new();
        for tx in txs {
            mempool.insert(txid(&tx), tx);
        }
        MockView {
            mempool,
            blocks: HashMap::new(),
        }
    }
}

impl BlockchainView for MockView {
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

    fn headers_from_height(
        &self,
        _from_height: u64,
        _max: usize,
    ) -> Vec<strangecoin_core::types::BlockHeader> {
        Vec::new()
    }

    fn blocks_by_hashes(&self, _hashes: &[[u8; 32]], _max: usize) -> Vec<Block> {
        Vec::new()
    }
}

/// Rebuilt block must carry a `tx_root` equal to the merkle root of the
/// transactions it actually holds — the anti-false-block invariant.
fn assert_tx_root_consistent(rebuilt: &Block) {
    let ids: Vec<[u8; 32]> = rebuilt.transactions.iter().map(txid).collect();
    assert_eq!(
        merkle_root(&ids),
        rebuilt.tx_root,
        "rebuilt tx_root does not match the transactions it carries"
    );
}

fuzz_target!(|input: CompactInput| {
    // Deduplicated body nonces → unique txids → unique short IDs.
    let mut nonces: Vec<u64> = input.nonces.iter().copied().take(MAX_BODY_TXS).collect();
    let mut seen = HashSet::new();
    nonces.retain(|n| seen.insert(*n));

    let mut txs = vec![coinbase_tx()];
    txs.extend(nonces.iter().map(|n| transfer_tx(*n)));
    let block = make_block(txs);
    let compact = CompactBlock::from_block(&block);

    // Receiver mempool: every body tx except the ones flagged missing.
    let receiver_txs: Vec<Transaction> = nonces
        .iter()
        .enumerate()
        .filter(|(i, _)| !input.missing.get(*i).copied().unwrap_or(false))
        .map(|(_, n)| transfer_tx(*n))
        .collect();
    let receiver = MockView::with_txs(receiver_txs);

    // --- honest path ------------------------------------------------------
    match fill_up(&compact, &receiver).expect("from_block output never errors") {
        FillOutcome::Complete(rebuilt) => {
            assert_eq!(rebuilt.hash, block.hash);
            assert_eq!(rebuilt.tx_root, block.tx_root);
            assert_eq!(rebuilt.transactions.len(), block.transactions.len());
            for (a, b) in rebuilt.transactions.iter().zip(&block.transactions) {
                assert_eq!(txid(a), txid(b));
            }
            assert_tx_root_consistent(&rebuilt);
        }
        FillOutcome::Missing(req) => {
            assert!(!req.missing_indices.is_empty(), "Missing with no indices");
            for index in &req.missing_indices {
                assert!(
                    *index < compact.short_ids.len(),
                    "requested index {index} out of range"
                );
            }
            // Round-trip against the broadcaster completes the block.
            let mut broadcaster = MockView::default();
            broadcaster.blocks.insert(block.hash.clone(), block.clone());
            let txn =
                handle_get_block_txn(&req, &broadcaster).expect("broadcaster holds the block");
            let rebuilt = apply_block_txn(&compact, &req.missing_indices, &txn, &receiver)
                .expect("honest round-trip applies");
            assert_eq!(rebuilt.hash, block.hash);
            assert_tx_root_consistent(&rebuilt);
        }
    }

    // --- corruption path: flip wire bytes, parse, fill up -----------------
    let mut wire = GossipMessage::CompactBlock(compact)
        .encode()
        .expect("encodes");
    for (i, value) in input.corruptions.iter().take(MAX_CORRUPTIONS).enumerate() {
        let pos = (value.wrapping_add(i as u8) as usize) % wire.len().max(1);
        if let Some(byte) = wire.get_mut(pos) {
            *byte ^= 0xFF;
        }
    }
    if let Ok(GossipMessage::CompactBlock(tampered)) = GossipMessage::parse(&wire) {
        match fill_up(&tampered, &receiver) {
            Ok(FillOutcome::Complete(rebuilt)) => {
                // The only acceptable "success" on tampered input: the
                // committed header still matches the txs it carries.
                assert_tx_root_consistent(&rebuilt);
            }
            // Missing or a typed error — both are clean fallbacks.
            Ok(FillOutcome::Missing(_)) | Err(_) => {}
        }
    }
});
