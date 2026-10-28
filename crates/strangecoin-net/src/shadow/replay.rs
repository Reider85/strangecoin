//! Mainnet block replay against a staging chain (S2-P20, Stage 6 prep).
//!
//! `replay_blocks` loads a mainnet block history from a file and submits
//! each block, in order, through [`BlockchainView::add_block`] — the same
//! validation entry point a live peer block goes through. Every block in
//! the file is mainnet canon by construction (it was exported from a
//! chain that accepted it), so any staging rejection is a consensus
//! divergence: the report carries [`DivergenceReport`]s produced by
//! [`compare`](super::compare::compare), and each one is logged via
//! `tracing::warn!(target: "net::shadow")`.
//!
//! ## Blocks-file format (deviation — the S2-P20 spec does not fix one)
//!
//! JSON, in either of two shapes, auto-detected: a JSON array of
//! [`Block`]s (`[{...},{...}]`) or JSON-lines (one `Block` per line,
//! blank lines and `#` comments ignored). `Block` derives serde, so an
//! export is `serde_json::to_string(&chain)` or one
//! `serde_json::to_string(&block)` per line.
//!
//! ## `staging_node_url` (deviation)
//!
//! [`ReplayConfig::staging_node_url`] is stored and logged for context
//! only: the function signature replays through an in-process
//! [`BlockchainView`], and the workspace has no HTTP client. The CLI
//! opens a local staging facade (`--data-dir`/`--chain-id`) and passes
//! it here; wiring a real remote transport is a Stage 4 CLI concern.
//!
//! Determinism (voluntary Stage 2 checklist review): the replay loop is
//! a pure fold over the file's block order — no `SystemTime`/`Instant`,
//! no RNG, no I/O beyond the one-shot file load.

use std::path::{Path, PathBuf};

use strangecoin_core::types::Block;

use crate::blockchain_view::BlockchainView;
use crate::error::NetError;
use crate::shadow::compare::{compare, BlockResult, BlockStatus, DivergenceReport};

/// Parameters of one shadow replay run (S2-P20).
#[derive(Debug, Clone, Default)]
pub struct ReplayConfig {
    /// Staging endpoint for logging/context only — see module docs.
    pub staging_node_url: String,
    /// Path to the exported mainnet block history (JSON array or
    /// JSON-lines, see module docs).
    pub mainnet_blocks_file: PathBuf,
    /// Stop at the first consensus divergence instead of replaying the
    /// whole file.
    pub stop_on_divergence: bool,
    /// Upper bound on blocks replayed; `0` means unlimited.
    pub max_blocks: usize,
}

/// Outcome of [`replay_blocks`]: per-block staging results plus every
/// mainnet-accepted / staging-rejected divergence found along the way.
#[derive(Debug, Clone, Default)]
pub struct ReplayReport {
    /// Blocks parsed from the file (before `max_blocks` truncation).
    pub total_loaded: usize,
    /// Blocks actually submitted to the staging view.
    pub replayed: usize,
    pub accepted: usize,
    pub rejected: usize,
    /// Divergences: accepted on mainnet (by file construction) but
    /// rejected on staging. Empty = staging agrees with mainnet history.
    pub divergences: Vec<DivergenceReport>,
    /// Staging outcome per submitted block, in file order.
    pub results: Vec<BlockResult>,
    /// Replay halted early because of `stop_on_divergence`.
    pub stopped_early: bool,
}

/// Load mainnet blocks from `path` (JSON array or JSON-lines).
pub fn load_blocks_file(path: &Path) -> Result<Vec<Block>, NetError> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        NetError::Replay(format!(
            "failed to read blocks file {}: {err}",
            path.display()
        ))
    })?;
    // Tolerate a UTF-8 BOM (Windows tooling writes one); U+FEFF is not
    // Unicode whitespace, so `trim_start` alone would not skip it.
    let content = raw.trim_start_matches('\u{feff}');
    let trimmed = content.trim_start();
    if trimmed.starts_with('[') {
        let blocks: Vec<Block> = serde_json::from_str(trimmed).map_err(|err| {
            NetError::Replay(format!(
                "failed to parse blocks file {}: {err}",
                path.display()
            ))
        })?;
        return Ok(blocks);
    }
    let mut blocks = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let block: Block = serde_json::from_str(line).map_err(|err| {
            NetError::Replay(format!(
                "failed to parse {} line {}: {err}",
                path.display(),
                idx + 1
            ))
        })?;
        blocks.push(block);
    }
    Ok(blocks)
}

/// Replay the mainnet history in `config.mainnet_blocks_file` through
/// `staging`, capturing accept/reject per block and detecting consensus
/// divergence (S2-P20).
pub async fn replay_blocks(
    config: &ReplayConfig,
    staging: &dyn BlockchainView,
) -> Result<ReplayReport, NetError> {
    let blocks = load_blocks_file(&config.mainnet_blocks_file)?;
    let total_loaded = blocks.len();
    let limit = if config.max_blocks == 0 {
        total_loaded
    } else {
        config.max_blocks.min(total_loaded)
    };

    tracing::info!(
        target: "net::shadow",
        staging_url = %config.staging_node_url,
        total_loaded,
        limit,
        "starting mainnet block replay"
    );

    let mut results: Vec<BlockResult> = Vec::with_capacity(limit);
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut stopped_early = false;

    for block in blocks.into_iter().take(limit) {
        let height = block.index;
        let hash = block.hash.clone();
        let status = match staging.add_block(block) {
            Ok(()) => {
                accepted += 1;
                BlockStatus::Accepted
            }
            Err(reason) => {
                rejected += 1;
                tracing::warn!(
                    target: "net::shadow",
                    block_height = height,
                    hash = %hash,
                    reason = %reason,
                    "block rejected on staging replay"
                );
                results.push(BlockResult {
                    height,
                    hash,
                    status: BlockStatus::Rejected,
                    reason: Some(reason),
                });
                if config.stop_on_divergence {
                    stopped_early = true;
                    break;
                }
                continue;
            }
        };
        results.push(BlockResult {
            height,
            hash,
            status,
            reason: None,
        });
    }

    // Every block in the file was accepted on mainnet by construction,
    // so the mainnet side of the comparison is uniform.
    let mainnet_results: Vec<BlockResult> = results
        .iter()
        .map(|r| BlockResult {
            height: r.height,
            hash: r.hash.clone(),
            status: BlockStatus::Accepted,
            reason: None,
        })
        .collect();
    let divergences = compare(&mainnet_results, &results);

    Ok(ReplayReport {
        total_loaded,
        replayed: results.len(),
        accepted,
        rejected,
        divergences,
        results,
        stopped_early,
    })
}
