//! AI-P03 — ACP handler fuzzing (invariants #24/#25, ADR-0030).
//!
//! Contract: `AcpHandler::handle` never panics on arbitrary bytes, and its
//! outcome classification is a pure function of (input, fresh handler
//! state): two independent handlers fed the same payload produce the same
//! `ok:<kind>` / `err:<AcpReject>` string. A panic is a crash; a typed
//! reject is a pass.
//!
//! Redelivery semantics follow the ADR-0030 §6.2 **write-ahead** rule: the
//! nonce lands in the store *before* the message body is processed, so
//! every outcome past authentication (accept, escrow failure, settlement
//! failure, local config fault, even a `Replay` verdict itself) leaves the
//! window at-or-above the frame's nonce — redelivery classifies as
//! `Replay`. Only *pre-auth* rejections (frame decode, envelope signature,
//! non-party sender) and nonce-store faults touch no state and must
//! repeat their exact class.
//!
//! Lanes over the fuzz bytes:
//! * **raw** — the payload goes straight to `handle` (decoder robustness:
//!   oversize frames, bad JSON, tag/kind mismatches, unknown tags);
//! * **signed** — a well-formed envelope (kind from the input, fuzz-derived
//!   counterparty/nonce/txid/signatures, fixed agent-A identity) signed
//!   with a fixed secp256k1 key, exercising nonce, party, escrow (#24 —
//!   the mock view has no chain, so every escrow reference lands on
//!   `MissingEscrowTx`) and signature paths;
//! * **tampered** — the signed wire with fuzz-patched bytes (envelope
//!   signature rejection).
//!
//! The view is a static mock (no RocksDB, no clock): the fuzz budget goes
//! through decode → authenticate → nonce → validate. Peer scoring runs on a
//! fresh in-memory `PeerManager` (`network.json` disabled).

use std::collections::HashMap;
use std::sync::Arc;

use secp256k1::{PublicKey, Secp256k1, SecretKey};
use strangecoin_core::address::encode_address;
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::types::{AccountState, Block, BlockHeader, ChainSnapshot, Transaction};
use strangecoin_net::discovery::addr_gossip::AddrEntry;
use strangecoin_net::{
    AcpEnvelope, AcpHandler, AcpHandlerConfig, AcpMessage, AcpNegotiate, AcpRevoke, AcpSettle,
    AcpTask, AgentId, BlockchainView, MemoryNonceStore, NoopEscrowVault, NodeId, NodeKeypair,
    PeerManager, PeerManagerConfig,
};

const SK_A: [u8; 32] = [0xA7; 32];

fn sk_a() -> SecretKey {
    SecretKey::from_slice(&SK_A).expect("fixed seed")
}

fn id_a() -> AgentId {
    AgentId(PublicKey::from_secret_key(&Secp256k1::new(), &sk_a()).serialize())
}

/// Counterparty identity carved from fuzz bytes: half the time agent B's
/// fixed key (parseable), half a raw fuzz blob (may be an invalid pubkey —
/// the escrow/party paths must handle both without panicking).
fn counterparty(data: &[u8]) -> AgentId {
    if data.first().copied().unwrap_or(0) % 2 == 0 {
        AgentId(PublicKey::from_secret_key(&Secp256k1::new(), &{
            SecretKey::from_slice(&[0xB6; 32]).expect("fixed seed")
        })
        .serialize())
    } else {
        let mut raw = [0x02u8; 33];
        for (slot, byte) in raw.iter_mut().zip(data.iter()) {
            *slot = *byte;
        }
        AgentId(raw)
    }
}

/// Static mock chain: genesis+one block, no transactions — every escrow
/// txid reference resolves to `MissingEscrowTx` (invariant #24's reject
/// path; the positive path lives in `tests/acp_messages.rs`).
struct MockView {
    blocks: Vec<Block>,
}

impl MockView {
    fn new() -> Self {
        let make = |index: u64, prev: &str| Block {
            index,
            timestamp: 1_700_000_000 + index,
            transactions: vec![],
            previous_hash: prev.to_string(),
            hash: format!("{index:064x}"),
            nonce: index,
            target: "ff".repeat(32),
            consensus_version: 1,
            state_root: [index as u8; 32],
            tx_root: [0u8; 32],
            receipts_root: [0u8; 32],
        };
        Self {
            blocks: vec![make(0, &"0".repeat(64)), make(1, &"0".repeat(64))],
        }
    }
}

impl BlockchainView for MockView {
    fn tip_hash(&self) -> String {
        self.blocks.last().map(|b| b.hash.clone()).unwrap_or_default()
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
    fn mempool_contains(&self, _txid: &[u8; 32]) -> bool {
        false
    }
    fn mempool_tx(&self, _txid: &[u8; 32]) -> Option<Transaction> {
        None
    }
    fn mempool_txs(&self) -> Vec<Transaction> {
        Vec::new()
    }
    fn block_by_hash(&self, block_hash: &str) -> Option<Block> {
        self.blocks.iter().find(|b| b.hash == block_hash).cloned()
    }
    fn save_state(&self) {}
    fn chain_contains_block(&self, block_hash: &str) -> bool {
        self.blocks.iter().any(|b| b.hash == block_hash)
    }
    fn snapshot_wire(&self) -> ChainSnapshot {
        ChainSnapshot {
            chain: self.blocks.clone(),
            balances: HashMap::new(),
            difficulty: 0,
            mempool_txs: Vec::new(),
            total_work: strangecoin_core::consensus::U256::default(),
        }
    }
    fn headers_from_height(&self, from_height: u64, max: usize) -> Vec<BlockHeader> {
        let mut headers: Vec<BlockHeader> = self
            .blocks
            .iter()
            .filter(|b| b.index >= from_height)
            .map(|b| b.header())
            .collect();
        headers.sort_by_key(|h| h.index);
        headers.truncate(max);
        headers
    }
    fn blocks_by_hashes(&self, hashes: &[[u8; 32]], max: usize) -> Vec<Block> {
        let mut out = Vec::new();
        for hash in hashes {
            if out.len() >= max {
                break;
            }
            if let Some(block) = self
                .blocks
                .iter()
                .find(|b| b.hash.as_bytes() == hash.as_slice())
            {
                out.push(block.clone());
            }
        }
        out
    }
    // get_tx: default `None` — the mock holds no transactions.
}

fn make_handler() -> (AcpHandler, NodeId) {
    let peer = NodeKeypair::from_seed([0x51; 32]).node_id();
    let pm = Arc::new(PeerManager::new(
        PeerManagerConfig::default(),
        NodeKeypair::from_seed([0x52; 32]),
    ));
    pm.learn_addrs(vec![AddrEntry {
        node_id: peer,
        addr: "10.77.0.9:8081".parse().expect("addr"),
    }]);
    let view: Arc<dyn BlockchainView> = Arc::new(MockView::new());
    // A plausible regtest contract address (never reached — `get_tx` is
    // always `None`, the existence check fires first).
    let contract = encode_address(
        &PublicKey::from_secret_key(&Secp256k1::new(), &sk_a()),
        CHAIN_ID_REGTEST,
    )
    .expect("address");
    let handler = AcpHandler::new(
        view,
        Arc::new(MemoryNonceStore::new()),
        Arc::new(NoopEscrowVault),
        pm,
        AcpHandlerConfig {
            network_id: CHAIN_ID_REGTEST,
            escrow_contract: Some(contract),
        },
    );
    (handler, peer)
}

/// Outcome classification — the deterministic string compared across runs.
fn classify(result: Result<strangecoin_net::AcpAccepted, strangecoin_net::AcpReject>) -> String {
    match result {
        Ok(accepted) => format!("ok:{:?}", accepted.kind),
        Err(reject) => format!("err:{reject:?}"),
    }
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    let mut buf = [0u8; 8];
    for (slot, byte) in buf.iter_mut().zip(data.get(at..).unwrap_or(&[])) {
        *slot = *byte;
    }
    u64::from_le_bytes(buf)
}

/// A well-formed envelope for lane `pick`, signed by agent A.
fn signed_payload(data: &[u8]) -> Vec<u8> {
    let a = id_a();
    let b = counterparty(data);
    let nonce = u64_at(data, 1) % 1_000;
    let mut txid = [0u8; 32];
    for (slot, byte) in txid.iter_mut().zip(data.get(9..).unwrap_or(&[])) {
        *slot = *byte;
    }
    let mut sig64 = [0x11u8; 64];
    for (slot, byte) in sig64.iter_mut().zip(data.get(41..).unwrap_or(&[])) {
        *slot = *byte;
    }
    let task_spec = serde_json::Value::String(String::from_utf8_lossy(data).to_string());

    let message = match data.first().copied().unwrap_or(0) % 5 {
        0 => AcpMessage::Negotiate(AcpNegotiate {
            agent_id_a: a,
            agent_id_b: b,
            agent_nonce: nonce,
            task_spec,
            quote: None,
        }),
        1 => AcpMessage::Negotiate(AcpNegotiate {
            agent_id_a: a,
            agent_id_b: b,
            agent_nonce: nonce,
            task_spec,
            quote: Some(strangecoin_net::AcpQuote {
                amount_sc: u64_at(data, 2),
                deadline_blocks: u64_at(data, 10) % 100_000,
            }),
        }),
        2 => AcpMessage::Task(AcpTask {
            agent_id_a: a,
            agent_id_b: b,
            agent_nonce: nonce,
            task_spec,
            escrow_txid: txid,
            deadline_block: u64_at(data, 3) % 100_000,
        }),
        3 => AcpMessage::Settle(AcpSettle {
            agent_id_a: a,
            agent_id_b: b,
            agent_nonce: nonce,
            escrow_txid: txid,
            settle_signature_a: sig64,
            settle_signature_b: {
                let mut other = sig64;
                other[0] ^= 0xFF;
                other
            },
        }),
        _ => AcpMessage::Revoke(AcpRevoke {
            agent_id_a: a,
            agent_id_b: b,
            agent_nonce: nonce,
            escrow_txid: txid,
            revoke_signature_a: sig64,
            revoke_signature_b: (nonce % 2 == 0).then_some(sig64),
            deadline_passed: nonce % 3 == 0,
        }),
    };
    AcpEnvelope::sign(message, a, &sk_a())
        .expect("agent A always signs")
        .encode_wire()
}

fn run_case(data: &[u8]) {
    let lane = data.first().copied().unwrap_or(0) % 3;
    let payload = match lane {
        // Raw decoder lane: fuzz bytes are the wire frame verbatim.
        0 => data.to_vec(),
        // Signed lane: well-formed envelope with fuzz-derived fields.
        1 => signed_payload(data.get(1..).unwrap_or(&[])),
        // Tampered lane: flip a byte inside a signed frame.
        _ => {
            let mut wire = signed_payload(data.get(1..).unwrap_or(&[]));
            if let Some(idx) = data.get(1).map(|b| usize::from(*b) % wire.len().max(1)) {
                if let Some(byte) = wire.get_mut(idx) {
                    *byte ^= 0x5A;
                }
            }
            wire
        }
    };

    // Fresh handler ×2: classification must be identical (pure function of
    // input + fresh state — the determinism half of the contract).
    let (handler_one, peer) = make_handler();
    let (handler_two, _) = make_handler();
    let first = classify(handler_one.handle(&payload, peer));
    let second = classify(handler_two.handle(&payload, peer));
    assert_eq!(
        first, second,
        "handle classification must not depend on handler identity/state"
    );

    // Redelivery through the same handler (invariant #25 + ADR-0030 §6.2
    // write-ahead): a frame that got past authentication consumed its
    // nonce and must now classify as `Replay`; a pre-auth rejection left
    // the window untouched and must classify identically.
    let repeat = classify(handler_one.handle(&payload, peer));
    let pre_auth = first.starts_with("err:Malformed")
        || first.starts_with("err:BadSignature")
        || first.starts_with("err:NotAParty")
        || first.starts_with("err:NonceStore");
    if pre_auth {
        assert_eq!(
            first, repeat,
            "pre-auth rejection must not consume the nonce window"
        );
    } else {
        assert!(
            repeat.starts_with("err:Replay"),
            "post-auth frame must be rejected as replay on redelivery, \
             first={first} repeat={repeat}"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    run_case(data);
});
