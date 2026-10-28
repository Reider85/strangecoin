//! S2-P20 — shadow network tooling: mainnet block replay against a
//! staging view + the consensus divergence detector.
//!
//! All simulated (no sockets, no LevelDB): a mock [`BlockchainView`]
//! accepts blocks unless their hash is in the rejection table, so each
//! test drives accept/reject outcomes with synthetic data. The three
//! S2-P20 acceptance tests are the first three below; the rest cover the
//! config flags and the blocks-file loader.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use strangecoin_core::serialize::header_hash;
use strangecoin_core::types::{AccountState, Block, BlockHeader, ChainSnapshot, Transaction};
use strangecoin_net::shadow::{
    compare, load_blocks_file, replay_blocks, BlockResult, BlockStatus, DivergenceReport,
    ReplayConfig,
};
use strangecoin_net::BlockchainView;

// ------------------------------------------------------------------ helpers

/// Synthetic chain block: header fields only, hash = `header_hash`.
fn synthetic_block(index: u64) -> Block {
    let mut header = BlockHeader {
        index,
        timestamp: 1_700_000_000 + index,
        previous_hash: if index == 0 {
            "0".repeat(64)
        } else {
            "0".repeat(64) // linkage is not replay's concern
        },
        hash: String::new(),
        nonce: index,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    header.hash = hex::encode(header_hash(&header));
    Block::from_header(header, Vec::new())
}

/// Staging view: blocks whose hex hash is a key of `rejects` are rejected
/// with that reason; everything else is accepted. Applied hashes land in
/// a Mutex (`&self` trait contract, same pattern as the S2-P19 TestView).
#[derive(Default)]
struct ReplayView {
    rejects: HashMap<String, String>,
    applied: Mutex<Vec<String>>,
}

impl ReplayView {
    fn new(rejects: HashMap<String, String>) -> Self {
        Self {
            rejects,
            applied: Mutex::new(Vec::new()),
        }
    }
}

impl BlockchainView for ReplayView {
    fn tip_hash(&self) -> String {
        self.applied
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_default()
    }
    fn chain_len(&self) -> usize {
        self.applied.lock().unwrap().len()
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
    fn add_block(&self, block: Block) -> Result<(), String> {
        if let Some(reason) = self.rejects.get(&block.hash) {
            return Err(reason.clone());
        }
        self.applied.lock().unwrap().push(block.hash);
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
            total_work: strangecoin_core::consensus::U256::default(),
        }
    }
    fn headers_from_height(&self, _from_height: u64, _max: usize) -> Vec<BlockHeader> {
        Vec::new()
    }
    fn blocks_by_hashes(&self, _hashes: &[[u8; 32]], _max: usize) -> Vec<Block> {
        Vec::new()
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("strangecoin-s2p20-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_blocks_json_lines(dir: &std::path::Path, blocks: &[Block]) -> PathBuf {
    let path = dir.join("blocks.jsonl");
    let mut out = String::new();
    for block in blocks {
        out.push_str(&serde_json::to_string(block).expect("serialize block"));
        out.push('\n');
    }
    std::fs::write(&path, out).expect("write blocks file");
    path
}

fn config_for(blocks_file: PathBuf) -> ReplayConfig {
    ReplayConfig {
        staging_node_url: "local://shadow".to_string(),
        mainnet_blocks_file: blocks_file,
        stop_on_divergence: false,
        max_blocks: 0,
    }
}

fn accepted(height: u64, hash: &str) -> BlockResult {
    BlockResult {
        height,
        hash: hash.to_string(),
        status: BlockStatus::Accepted,
        reason: None,
    }
}

fn rejected(height: u64, hash: &str, reason: &str) -> BlockResult {
    BlockResult {
        height,
        hash: hash.to_string(),
        status: BlockStatus::Rejected,
        reason: Some(reason.to_string()),
    }
}

// ------------------------------------------------- required S2-P20 tests

/// Known-valid mainnet blocks replayed on staging → accepted, no divergence.
#[tokio::test]
async fn replay_of_known_valid_blocks_is_accepted_without_divergence() {
    let dir = temp_dir("valid");
    let blocks: Vec<Block> = (0..3).map(synthetic_block).collect();
    let file = write_blocks_json_lines(&dir, &blocks);

    let view = ReplayView::default();
    let report = replay_blocks(&config_for(file), &view)
        .await
        .expect("replay");

    assert_eq!(report.total_loaded, 3);
    assert_eq!(report.replayed, 3);
    assert_eq!(report.accepted, 3);
    assert_eq!(report.rejected, 0);
    assert!(report.divergences.is_empty(), "no divergence expected");
    assert!(!report.stopped_early);
    assert!(report
        .results
        .iter()
        .all(|r| r.status == BlockStatus::Accepted));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A block rejected on both mainnet and staging with the same reason is
/// NOT a divergence (compare-level, mock data).
#[test]
fn compare_reports_no_divergence_when_both_sides_reject_same_reason() {
    let block = synthetic_block(7);
    let hash = block.hash.as_str();
    let mainnet = vec![rejected(7, hash, "invalid pow: target not met")];
    let staging = vec![rejected(7, hash, "invalid pow: target not met")];

    let divergences = compare(&mainnet, &staging);
    assert!(
        divergences.is_empty(),
        "same-reason rejection is not a divergence"
    );
}

/// Block accepted on mainnet but rejected on staging → detector emits a
/// divergence report (and a tracing warning).
#[test]
fn compare_detects_divergence_on_status_mismatch() {
    let block = synthetic_block(11);
    let hash = block.hash.as_str();
    let mainnet = vec![accepted(11, hash)];
    let staging = vec![rejected(11, hash, "state root mismatch")];

    let divergences = compare(&mainnet, &staging);
    assert_eq!(divergences.len(), 1);
    assert_eq!(
        divergences[0],
        DivergenceReport {
            block_height: 11,
            mainnet_status: BlockStatus::Accepted,
            staging_status: BlockStatus::Rejected,
            divergence_reason: Some("state root mismatch".to_string()),
        }
    );
}

// ------------------------------------------------------- replay-path extras

/// Replay path: staging rejects one mainnet-accepted block → the report
/// carries the divergence.
#[tokio::test]
async fn replay_flags_divergence_when_staging_rejects_mainnet_block() {
    let dir = temp_dir("divergence");
    let blocks: Vec<Block> = (0..3).map(synthetic_block).collect();
    let file = write_blocks_json_lines(&dir, &blocks);

    let mut rejects = HashMap::new();
    rejects.insert(blocks[1].hash.clone(), "state root mismatch".to_string());
    let view = ReplayView::new(rejects);

    let report = replay_blocks(&config_for(file), &view)
        .await
        .expect("replay");

    assert_eq!(report.accepted, 2);
    assert_eq!(report.rejected, 1);
    assert_eq!(report.divergences.len(), 1);
    let div = &report.divergences[0];
    assert_eq!(div.block_height, blocks[1].index);
    assert_eq!(div.mainnet_status, BlockStatus::Accepted);
    assert_eq!(div.staging_status, BlockStatus::Rejected);
    assert_eq!(
        div.divergence_reason.as_deref(),
        Some("state root mismatch")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `stop_on_divergence` halts at the first rejected block.
#[tokio::test]
async fn stop_on_divergence_halts_replay_at_first_rejection() {
    let dir = temp_dir("stop");
    let blocks: Vec<Block> = (0..3).map(synthetic_block).collect();
    let file = write_blocks_json_lines(&dir, &blocks);

    let mut rejects = HashMap::new();
    rejects.insert(blocks[0].hash.clone(), "bad block".to_string());
    let view = ReplayView::new(rejects);

    let mut config = config_for(file);
    config.stop_on_divergence = true;
    let report = replay_blocks(&config, &view).await.expect("replay");

    assert!(report.stopped_early);
    assert_eq!(report.replayed, 1);
    assert_eq!(report.rejected, 1);
    assert_eq!(report.divergences.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `max_blocks` truncates the replay; `0` means unlimited.
#[tokio::test]
async fn max_blocks_caps_replay_and_zero_means_unlimited() {
    let dir = temp_dir("max");
    let blocks: Vec<Block> = (0..5).map(synthetic_block).collect();
    let file = write_blocks_json_lines(&dir, &blocks);

    let mut config = config_for(file.clone());
    config.max_blocks = 2;
    let report = replay_blocks(&config, &ReplayView::default())
        .await
        .expect("replay capped");
    assert_eq!(report.total_loaded, 5);
    assert_eq!(report.replayed, 2);
    assert_eq!(report.accepted, 2);

    let report = replay_blocks(&config_for(file), &ReplayView::default())
        .await
        .expect("replay unlimited");
    assert_eq!(report.replayed, 5);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ loader tests

#[test]
fn load_blocks_file_parses_json_array_and_json_lines() {
    let dir = temp_dir("loader");
    let blocks: Vec<Block> = (0..2).map(synthetic_block).collect();

    // JSON array form.
    let array_path = dir.join("blocks.json");
    std::fs::write(
        &array_path,
        serde_json::to_string_pretty(&blocks).expect("serialize"),
    )
    .expect("write array file");
    let loaded = load_blocks_file(&array_path).expect("load array");
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[1].hash, blocks[1].hash);

    // JSON-lines form with a comment, a blank line and a UTF-8 BOM
    // (PowerShell 5.1 `Out-File -Encoding utf8` writes one).
    let lines_path = dir.join("blocks.jsonl");
    let mut content = String::from("\u{feff}# mainnet export\n\n");
    for block in &blocks {
        content.push_str(&serde_json::to_string(block).expect("serialize"));
        content.push('\n');
    }
    std::fs::write(&lines_path, content).expect("write lines file");
    let loaded = load_blocks_file(&lines_path).expect("load lines");
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].hash, blocks[0].hash);

    // Missing file → NetError::Replay.
    let missing = dir.join("nope.json");
    let err = load_blocks_file(&missing).expect_err("missing file must fail");
    assert!(matches!(err, strangecoin_net::NetError::Replay(_)));

    let _ = std::fs::remove_dir_all(&dir);
}
