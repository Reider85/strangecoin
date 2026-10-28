//! S2-P19 — async migration guards.
//!
//! 1. **Source scan**: `strangecoin-net/src` must contain no dedicated-thread
//!    or blocking-std networking primitives — the whole network stack runs
//!    on tokio (ADR-0011 continuation). `std::net::SocketAddr` (a value
//!    type) stays allowed; only the blocking sockets are banned.
//! 2. **Spawn on runtime**: `sync_engine::spawn` inside `#[tokio::test]`
//!    schedules the engine as a task and end-to-end processing works.
//! 3. **Spawn without runtime panics**: the pre-S2-P19 dedicated-thread
//!    fallback is gone — a caller without a runtime gets a loud panic, not
//!    a silently extra thread.
//! 4. **Clean shutdown**: covered by the `sync_engine` unit test
//!    `run_stops_cleanly_on_shutdown_flag` (private field access) — this
//!    file's runtime tests exercise the public surface.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use strangecoin_core::consensus::U256;
use strangecoin_core::types::{AccountState, Block, ChainSnapshot, Transaction, TransferTx};
use strangecoin_net::sync_engine::{self, EventSink, Incoming};
use strangecoin_net::{BlockchainView, RateLimiter};

// -------------------------------------------------------------- source scan

/// Substrings that must not appear in `strangecoin-net/src` (S2-P19: the
/// whole network stack is tokio). Kept as exact paths so a legitimate
/// mention of `SocketAddr` or `tokio::sync::mpsc` never trips the scan.
const BANNED: &[&str] = &[
    "std::thread::spawn",
    "std::thread::Builder",
    "std::thread::sleep",
    "std::sync::mpsc",
    "std::net::TcpStream",
    "std::net::TcpListener",
];

fn rust_sources(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn network_sources_use_no_blocking_thread_or_std_socket_primitives() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    assert!(!files.is_empty(), "no Rust sources found under {src:?}");
    for file in files {
        let content = std::fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
        for banned in BANNED {
            assert!(
                !content.contains(banned),
                "{} contains banned primitive {:?} (S2-P19: network stack must be tokio)",
                file.display(),
                banned
            );
        }
    }
}

// ------------------------------------------------------- spawn on a runtime

/// Minimal view: enough for the engine to process one `NewTx`.
#[derive(Default)]
struct StubView {
    applied: Mutex<Vec<String>>,
}

impl BlockchainView for StubView {
    fn tip_hash(&self) -> String {
        "tip".to_string()
    }
    fn chain_len(&self) -> usize {
        1
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
    fn apply_tx(&self, transaction: Transaction) -> Result<(), String> {
        let txid = hex::encode(strangecoin_core::serialize::txid(&transaction));
        self.applied.lock().unwrap().push(txid);
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
            mempool_txs: Vec::new(),
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

/// EventSink capturing accepted txids.
#[derive(Default)]
struct CaptureSink {
    tx_accepted: Mutex<Vec<String>>,
}

impl EventSink for CaptureSink {
    fn block_applied(&self, _height: u64, _hash: String) {}
    fn block_reorged(&self, _old_tip: String, _new_tip: String) {}
    fn tx_accepted(&self, txid: String) {
        self.tx_accepted.lock().unwrap().push(txid);
    }
    fn tx_rejected(&self, _txid: String, _reason: String) {}
    fn state_persisted(&self, _height: u64) {}
}

fn sample_tx() -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "rsc1sender".into(),
        to: "rsc1receiver".into(),
        amount: 7,
        fee: 0,
        nonce: 1,
        chain_id: 3,
        signature: vec![1u8; 65],
        is_coinbase: false,
    })
}

#[tokio::test]
async fn spawn_on_runtime_processes_messages_as_a_task() {
    let view = Arc::new(StubView::default());
    let sink = Arc::new(CaptureSink::default());
    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let inbox = sync_engine::spawn(
        view.clone(),
        sink.clone(),
        sync_tx,
        Arc::clone(&shutdown),
        Arc::new(RateLimiter::default()),
    );

    let tx = sample_tx();
    let expected_txid = hex::encode(strangecoin_core::serialize::txid(&tx));
    assert!(inbox.push(Incoming::NewTx { tx, from: None }));

    // The engine task runs on the same (current-thread) runtime — yield
    // until it drains the lane.
    for _ in 0..50 {
        if !sink.tx_accepted.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    {
        let accepted = sink.tx_accepted.lock().unwrap();
        assert_eq!(
            accepted.as_slice(),
            [expected_txid.as_str()],
            "engine must apply the tx and publish TxAccepted"
        );
    }

    shutdown.store(true, Ordering::Relaxed);
    // The engine observes the flag on its next poll tick; give it a moment
    // before the runtime tears down.
    tokio::time::sleep(Duration::from_millis(300)).await;
}

// ---------------------------------------------------- spawn without runtime

#[test]
#[should_panic(expected = "must be called from the context of a Tokio 1.x runtime")]
fn spawn_without_runtime_panics() {
    // No runtime on this plain test thread: `tokio::spawn` panics with the
    // standard "no reactor running … Tokio 1.x runtime" message instead of
    // silently spawning a dedicated thread (the removed fallback).
    let view = Arc::new(StubView::default());
    let sink = Arc::new(CaptureSink::default());
    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();
    let _ = sync_engine::spawn(
        view,
        sink,
        sync_tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(RateLimiter::default()),
    );
}
