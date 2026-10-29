//! S2-P21 — gossip INV/GET_DATA handler fuzzing (S2-P13, threats V-14/V-17).
//!
//! Contract: arbitrary INV/GET_DATA sequences through `handle_inv` /
//! `handle_get_data` never panic; `handle_inv` only *requests* items the
//! local view lacks (never answers itself), and `handle_get_data` only
//! *serves* transactions genuinely present in the mempool — an INV-announced
//! txid is never answered with a fabricated TX (mempool consistency, no
//! false-positive announces). Raw INV bytes through the codec surface typed
//! errors, never a panic.

#![no_main]

use std::collections::HashMap;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use strangecoin_core::consensus::U256;
use strangecoin_core::serialize::txid;
use strangecoin_core::types::{AccountState, Block, ChainSnapshot, Transaction, TransferTx};
use strangecoin_net::gossip::{handle_get_data, handle_inv};
use strangecoin_net::{
    BlockchainView, GetData, GossipMessage, GossipState, Inv, InvKind, InventoryItem, NodeId,
};

const MAX_OPS: usize = 48;
const MAX_MEMPOOL: usize = 8;
/// Bound for raw codec inputs.
const MAX_RAW: usize = 1024;

#[derive(Arbitrary, Debug)]
struct ItemSpec {
    /// When `true` the hash is the txid of `tx_with_nonce(nonce)` (may hit
    /// the mempool); when `false` the raw bytes are used (always a miss).
    from_nonce: bool,
    nonce: u64,
    raw: [u8; 32],
}

impl ItemSpec {
    fn hash(&self) -> [u8; 32] {
        if self.from_nonce {
            txid(&tx_with_nonce(self.nonce))
        } else {
            self.raw
        }
    }

    fn kind(&self, tx: bool) -> InvKind {
        if tx {
            InvKind::Tx
        } else {
            InvKind::Block
        }
    }
}

#[derive(Arbitrary, Debug)]
enum GossipOp {
    /// A peer announces one item.
    Announce { tx: bool, item: ItemSpec },
    /// A peer requests one item.
    Request { tx: bool, item: ItemSpec },
    /// Raw INV bytes through the codec.
    RawInv { bytes: Vec<u8> },
}

#[derive(Arbitrary, Debug)]
struct InvInput {
    /// Nonces of the transactions seeded into the local mempool.
    mempool_nonces: Vec<u64>,
    /// Identifies the announcing peer (state key).
    peer_id: [u8; 32],
    ops: Vec<GossipOp>,
}

fn tx_with_nonce(nonce: u64) -> Transaction {
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

/// Minimal in-memory `BlockchainView`: a controllable mempool and an empty
/// chain (block announces are therefore always "missing" — the GET_DATA
/// request path, never a served BLOCK).
struct MockView {
    mempool: HashMap<[u8; 32], Transaction>,
}

impl MockView {
    fn with_txs(txs: Vec<Transaction>) -> Self {
        let mut mempool = HashMap::new();
        for tx in txs {
            mempool.insert(txid(&tx), tx);
        }
        MockView { mempool }
    }
}

impl BlockchainView for MockView {
    fn tip_hash(&self) -> String {
        String::new()
    }

    fn chain_len(&self) -> usize {
        0
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

    fn block_by_hash(&self, _block_hash: &str) -> Option<Block> {
        None
    }

    fn save_state(&self) {}

    fn chain_contains_block(&self, _block_hash: &str) -> bool {
        false
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

fuzz_target!(|input: InvInput| {
    let mempool: Vec<Transaction> = input
        .mempool_nonces
        .iter()
        .take(MAX_MEMPOOL)
        .map(|nonce| tx_with_nonce(*nonce))
        .collect();
    let view = MockView::with_txs(mempool);
    let peer = NodeId::from_public_bytes(input.peer_id);
    let mut state = GossipState::new();

    for op in input.ops.iter().take(MAX_OPS) {
        match op {
            GossipOp::Announce { tx, item } => {
                let inv = Inv {
                    items: vec![InventoryItem {
                        kind: item.kind(*tx),
                        hash: item.hash(),
                    }],
                };
                match handle_inv(&inv, peer, &mut state, &view) {
                    Ok(messages) => {
                        for message in messages {
                            match message {
                                GossipMessage::GetData(get_data) => {
                                    for requested in &get_data.items {
                                        // Consistency: we only ask for what we lack.
                                        match requested.kind {
                                            InvKind::Tx => assert!(
                                                !view.mempool_contains(&requested.hash),
                                                "GET_DATA requested a tx the mempool already has"
                                            ),
                                            InvKind::Block => assert!(
                                                !view.chain_contains_block(&hex::encode(
                                                    requested.hash
                                                )),
                                                "GET_DATA requested a block the chain already has"
                                            ),
                                        }
                                    }
                                }
                                other => panic!("handle_inv answered with {other:?}"),
                            }
                        }
                    }
                    // Typed protocol errors are an acceptable outcome.
                    Err(_) => {}
                }
            }
            GossipOp::Request { tx, item } => {
                let get_data = GetData {
                    items: vec![InventoryItem {
                        kind: item.kind(*tx),
                        hash: item.hash(),
                    }],
                };
                for message in handle_get_data(&get_data, peer, &mut state, &view) {
                    match message {
                        GossipMessage::Tx(served) => {
                            assert!(
                                view.mempool_contains(&txid(&served)),
                                "handle_get_data served a TX that is not in the mempool"
                            );
                        }
                        GossipMessage::Block(served) => {
                            assert!(
                                view.chain_contains_block(&served.hash),
                                "handle_get_data served a block the chain does not have"
                            );
                        }
                        GossipMessage::NotFound(_) => {}
                        other => panic!("handle_get_data answered with {other:?}"),
                    }
                }
            }
            GossipOp::RawInv { bytes } => {
                let bytes: Vec<u8> = bytes.iter().copied().take(MAX_RAW).collect();
                if let Ok(inv) = Inv::parse(&bytes) {
                    // A parsed INV must be internally well-formed.
                    for item in &inv.items {
                        let _ = item.hash;
                        assert!(matches!(item.kind, InvKind::Tx | InvKind::Block));
                    }
                }
            }
        }
    }
});
