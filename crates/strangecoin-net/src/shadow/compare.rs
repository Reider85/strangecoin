//! Consensus divergence detector (S2-P20): compares per-block replay
//! outcomes from two chains (mainnet history vs a staging replay) and
//! reports every block whose accept/reject status differs.
//!
//! Matching is by block [`hash`](BlockResult::hash) — the replay path
//! always yields one result per loaded block on both sides, so a mainnet
//! entry without a staging counterpart is not a *status* divergence and
//! is skipped (documented deviation: the S2-P20 spec does not fix a
//! matching key). Every emitted divergence is also logged via
//! `tracing::warn!(target: "net::shadow")` — the "emits warning" half of
//! the S2-P20 acceptance criteria.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Outcome of submitting one block to a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockStatus {
    /// Block validated and appended.
    Accepted,
    /// Block rejected by validation; see [`BlockResult::reason`].
    Rejected,
}

/// Per-block replay outcome: identity (`height` + `hash`) plus status and
/// the rejection reason when rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockResult {
    pub height: u64,
    pub hash: String,
    pub status: BlockStatus,
    pub reason: Option<String>,
}

/// One status mismatch between the mainnet history and the staging
/// replay (S2-P20 divergence report).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DivergenceReport {
    pub block_height: u64,
    pub mainnet_status: BlockStatus,
    pub staging_status: BlockStatus,
    pub divergence_reason: Option<String>,
}

/// Find blocks whose status differs between `mainnet_results` and
/// `staging_results` (joined by block hash), logging a warning for each.
///
/// The divergence reason prefers the staging side's rejection reason and
/// falls back to the mainnet one, so the report always carries the most
/// specific signal available.
pub fn compare(
    mainnet_results: &[BlockResult],
    staging_results: &[BlockResult],
) -> Vec<DivergenceReport> {
    let staging_by_hash: HashMap<&str, &BlockResult> = staging_results
        .iter()
        .map(|r| (r.hash.as_str(), r))
        .collect();

    let mut divergences = Vec::new();
    for mainnet in mainnet_results {
        let Some(staging) = staging_by_hash.get(mainnet.hash.as_str()) else {
            continue;
        };
        if mainnet.status == staging.status {
            continue;
        }
        let divergence_reason = staging.reason.clone().or_else(|| mainnet.reason.clone());
        tracing::warn!(
            target: "net::shadow",
            block_height = mainnet.height,
            hash = %mainnet.hash,
            mainnet = ?mainnet.status,
            staging = ?staging.status,
            reason = divergence_reason.as_deref().unwrap_or(""),
            "consensus divergence detected"
        );
        divergences.push(DivergenceReport {
            block_height: mainnet.height,
            mainnet_status: mainnet.status,
            staging_status: staging.status,
            divergence_reason,
        });
    }
    divergences
}
