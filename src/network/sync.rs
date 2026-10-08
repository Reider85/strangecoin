//! Headers-first sync (S1-P16; ADR-0011: network loop is async).
//!
//! Three layers, deliberately separable so the logic is testable without TCP:
//!
//! 1. [`HeaderCache`] — ingest path: PoW per header (`validate_header_pow`),
//!    parent linkage, index continuity. A header that fails any of these is
//!    dropped and nothing after it in the batch is trusted.
//! 2. [`plan_best_branch`] — fork choice over headers only: candidate tips are
//!    walked back to the shared genesis, compared by cumulative work through
//!    [`ChainSelector`] (work → timestamp → hash) against the local chain.
//! 3. [`sync_headers_first`] — the network loop: GET_HEADERS until the peer's
//!    headers are exhausted, plan, GET_BLOCKS for exactly the missing hashes,
//!    and assemble the candidate chain (`local prefix + fork bodies`). Since
//!    ADR-0010 the loop does **not** adopt: the caller enqueues the candidate
//!    into the SyncEngine inbox, and the engine runs the full `validate_chain`
//!    path over the bodies. Headers lay out the route; they never weaken it.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::error::StrangecoinError;
use crate::network::protocol::{self, MAX_BLOCKS_BATCH, MAX_HEADERS_BATCH};
use strangecoin_core::chain_selector::{ChainInfo, ChainSelector};
use strangecoin_core::consensus::{cumulative_work_headers, validate_header_pow};
use strangecoin_core::serialize::block_hash;
use strangecoin_core::types::{Block, BlockHeader};

/// Read/write timeout for one headers-first exchange. A pre-P16 peer closes
/// the connection without answering a binary request, which surfaces here as
/// an EOF/timeout and triggers the GET_BLOCKCHAIN fallback.
pub const SYNC_IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Connect timeout for a single sync peer.
pub const SYNC_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

// ------------------------------------------------------------- header cache

/// Validated headers keyed by their (derived) hash. Insert order is recorded
/// implicitly through [`HeaderCache::max_index`].
#[derive(Default)]
pub struct HeaderCache {
    by_hash: HashMap<String, BlockHeader>,
    max_index: Option<u64>,
}

impl HeaderCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }

    pub fn max_index(&self) -> Option<u64> {
        self.max_index
    }

    pub fn get(&self, hash: &str) -> Option<&BlockHeader> {
        self.by_hash.get(hash)
    }

    pub fn headers(&self) -> impl Iterator<Item = &BlockHeader> {
        self.by_hash.values()
    }

    /// Ingest one batch (ascending order from the wire).
    ///
    /// Every header must pass `validate_header_pow`; the first header's
    /// parent must be the genesis (index 0), a header already in this cache,
    /// or a block of the local chain; every following header must link to its
    /// predecessor (`previous_hash` + `index + 1`).
    ///
    /// Returns how many headers were accepted: ingestion stops at the first
    /// violation, so a bad header is dropped together with everything after
    /// it and cannot influence tip selection. `0` for a non-empty batch means
    /// the peer is unusable.
    pub fn ingest(&mut self, batch: &[BlockHeader], local_hashes: &HashSet<String>) -> usize {
        let mut prev: Option<(String, u64)> = None;
        let mut accepted = 0usize;
        for header in batch {
            if validate_header_pow(header).is_err() {
                warn!(
                    index = header.index,
                    "Dropping header: PoW check failed"
                );
                break;
            }
            let linked = match &prev {
                Some((hash, index)) => {
                    header.previous_hash == *hash && header.index == *index + 1
                }
                None => {
                    header.index == 0
                        || self.by_hash.contains_key(&header.previous_hash)
                        || local_hashes.contains(&header.previous_hash)
                }
            };
            if !linked {
                warn!(
                    index = header.index,
                    prev = %header.previous_hash,
                    "Dropping header: broken link to parent"
                );
                break;
            }
            self.max_index = Some(match self.max_index {
                Some(m) => m.max(header.index),
                None => header.index,
            });
            self.by_hash.insert(header.hash.clone(), header.clone());
            prev = Some((header.hash.clone(), header.index));
            accepted += 1;
        }
        accepted
    }
}

// ------------------------------------------------------------- branch plan

/// A chosen candidate branch: the headers after the fork point plus where the
/// fork happens in the local chain (the local prefix through `fork_index`
/// stays; `headers` replace everything after it).
#[derive(Debug, Clone)]
pub struct BranchPlan {
    /// Index of the last local block shared with the candidate (`>= 0` —
    /// at minimum the genesis).
    pub fork_index: u64,
    /// Candidate headers strictly after `fork_index`, ascending.
    pub headers: Vec<BlockHeader>,
    /// Fork-choice info of the whole candidate chain (genesis..tip).
    pub candidate_info: ChainInfo,
}

/// Pick the best branch the header cache offers, if it beats the local chain.
///
/// Candidate tips are discovered as headers with no child in the cache; each
/// tip's ancestry is walked back to index 0 and must share the local genesis
/// (otherwise the branch is incompatible and skipped). The winner is chosen
/// by [`ChainSelector::is_better`] against the local chain's own
/// [`ChainInfo`] — identical rules to `adopt_candidate`, just computed on
/// headers before any body is downloaded.
pub fn plan_best_branch(local_chain: &[Block], cache: &HeaderCache) -> Option<BranchPlan> {
    if local_chain.is_empty() || cache.is_empty() {
        return None;
    }
    let local_info = ChainSelector::chain_info(local_chain)?;
    let local_genesis_hash = local_chain[0].hash.clone();

    // Tips: header hashes nobody in the cache points at as a parent.
    let parents: HashSet<&str> = cache
        .headers()
        .map(|h| h.previous_hash.as_str())
        .collect();
    let tips: Vec<&BlockHeader> = cache
        .headers()
        .filter(|h| !parents.contains(h.hash.as_str()))
        .collect();

    let mut best: Option<(Vec<BlockHeader>, ChainInfo)> = None;
    for tip in tips {
        let Some(ancestry) = walk_ancestry(cache, tip) else {
            continue;
        };
        // Compatible branches share the local genesis.
        if ancestry.first().map(|h| h.hash.as_str()) != Some(local_genesis_hash.as_str()) {
            debug!(tip = %tip.hash, "Skipping branch with foreign genesis");
            continue;
        }
        let info = ChainInfo {
            tip_height: tip.index,
            tip_hash: tip.hash.clone(),
            total_work: cumulative_work_headers(&ancestry),
            tip_timestamp: tip.timestamp,
        };
        let better = match &best {
            None => true,
            Some((_, current)) => ChainSelector::is_better(&info, current),
        };
        if better {
            best = Some((ancestry, info));
        }
    }

    let (ancestry, candidate_info) = best?;
    if !ChainSelector::is_better(&candidate_info, &local_info) {
        return None;
    }

    // Fork point: longest shared prefix by hash (genesis always matches for
    // compatible branches, so fork_index >= 0).
    let mut fork_index = 0u64;
    let mut shared = 0usize;
    for (hdr, local) in ancestry.iter().zip(local_chain.iter()) {
        if hdr.hash != local.hash {
            break;
        }
        fork_index = hdr.index;
        shared += 1;
    }
    // Defensive: shared prefix must be a prefix of the local chain too.
    if shared == 0 || shared > local_chain.len() {
        return None;
    }
    let headers = ancestry[shared..].to_vec();
    if headers.is_empty() {
        return None;
    }
    Some(BranchPlan {
        fork_index,
        headers,
        candidate_info,
    })
}

/// Header ancestry ending at `tip`, ascending, bounded by the cache size
/// (a cycle in the cache would otherwise loop forever).
fn walk_ancestry(cache: &HeaderCache, tip: &BlockHeader) -> Option<Vec<BlockHeader>> {
    let mut chain = Vec::new();
    let mut current = tip.clone();
    let mut steps = 0usize;
    loop {
        chain.push(current.clone());
        if current.index == 0 {
            break;
        }
        steps += 1;
        if steps > cache.len() {
            warn!("Header ancestry walk exceeded cache size (cycle?)");
            return None;
        }
        current = cache.get(&current.previous_hash)?.clone();
    }
    chain.reverse();
    Some(chain)
}

// ---------------------------------------------------------- network phase

#[derive(Debug, Clone, Default)]
pub struct SyncOutcome {
    /// Candidate chain assembled from the local prefix plus the fork bodies,
    /// when the peer's branch beats the local one (`None` = nothing better).
    /// The caller enqueues it into the SyncEngine inbox; adoption happens
    /// there (ADR-0010).
    pub candidate: Option<Vec<Block>>,
    /// Headers accepted by the ingest validation.
    pub headers_ingested: usize,
    /// Bodies downloaded for the chosen branch.
    pub blocks_downloaded: usize,
    /// Tip height of the candidate the peer advertised (0 when none).
    pub peer_tip_height: u64,
}

/// One headers-first sync round against a single peer (ADR-0011: async).
///
/// Errors mean "this peer could not serve headers-first" — the caller falls
/// back to the legacy `GET_BLOCKCHAIN` path (pre-P16 peers never answer a
/// binary request, so this is the compatibility trigger).
///
/// The loop is pure download + plan: it never touches the local chain beyond
/// reading `local_chain`, so handing the returned candidate to the engine is
/// the caller's job. Every socket operation is bounded by
/// [`SYNC_IO_TIMEOUT`]; the shutdown flag is polled between phases.
pub async fn sync_headers_first(
    addr: SocketAddr,
    network_id: u32,
    local_chain: &[Block],
    shutdown: &Arc<AtomicBool>,
) -> Result<SyncOutcome, StrangecoinError> {
    // tokio has no TcpStream::connect_timeout — bound the connect itself.
    let mut stream = timeout(SYNC_CONNECT_TIMEOUT, tokio::net::TcpStream::connect(&addr))
        .await
        .map_err(|_| timeout_error())??;
    timeout(SYNC_IO_TIMEOUT, stream.write_all(&protocol::encode_hello(network_id)))
        .await
        .map_err(|_| timeout_error())??;

    // Local view taken once: this round plans against one consistent snapshot.
    let local_hashes: HashSet<String> = local_chain.iter().map(|b| b.hash.clone()).collect();

    // Phase 1: header sync.
    let mut cache = HeaderCache::new();
    let mut from_height = 0u64;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return Err(shutdown_error());
        }
        with_io_timeout(protocol::write_length_prefixed_async(
            &mut stream,
            &protocol::encode_get_headers(from_height),
        ))
        .await?;
        let payload = with_io_timeout(protocol::read_length_prefixed_async(&mut stream)).await?;
        let headers = protocol::parse_headers(&payload)?;
        if headers.is_empty() {
            break;
        }
        let batch_len = headers.len();
        let accepted = cache.ingest(&headers, &local_hashes);
        if accepted == 0 {
            return Err(StrangecoinError::InvalidBlock(
                "headers-first: peer sent no valid header".to_string(),
            ));
        }
        if batch_len < MAX_HEADERS_BATCH || accepted < batch_len {
            // Peer exhausted its chain, or we stopped it at a bad header.
            break;
        }
        from_height = cache.max_index().map_or(0, |m| m + 1);
    }
    debug!(peer = %addr, headers = cache.len(), "Headers ingested");

    // Phase 2: fork choice over headers.
    let Some(plan) = plan_best_branch(local_chain, &cache) else {
        return Ok(SyncOutcome {
            candidate: None,
            headers_ingested: cache.len(),
            blocks_downloaded: 0,
            peer_tip_height: cache.max_index().unwrap_or(0),
        });
    };
    info!(
        peer = %addr,
        fork_index = plan.fork_index,
        to_download = plan.headers.len(),
        candidate_tip = plan.candidate_info.tip_height,
        "Chose headers-first branch"
    );

    // Phase 3: bodies for exactly the missing hashes, batched and bounded.
    let mut blocks: Vec<Block> = Vec::with_capacity(plan.headers.len());
    for chunk in plan.headers.chunks(MAX_BLOCKS_BATCH) {
        if shutdown.load(Ordering::Relaxed) {
            return Err(shutdown_error());
        }
        let mut requested: Vec<[u8; 32]> = Vec::with_capacity(chunk.len());
        for header in chunk {
            let bytes =
                hex::decode(&header.hash).map_err(|_| StrangecoinError::InvalidBlock(
                    "headers-first: non-hex header hash".to_string(),
                ))?;
            let arr: [u8; 32] = bytes.try_into().map_err(|_| {
                StrangecoinError::InvalidBlock("headers-first: bad header hash".to_string())
            })?;
            requested.push(arr);
        }
        with_io_timeout(protocol::write_length_prefixed_async(
            &mut stream,
            &protocol::encode_get_blocks(&requested)?,
        ))
        .await?;
        let payload = with_io_timeout(protocol::read_length_prefixed_async(&mut stream)).await?;
        let batch = protocol::parse_blocks(&payload)?;
        let expected: HashSet<String> = chunk.iter().map(|h| h.hash.clone()).collect();
        let mut seen: HashSet<String> = HashSet::new();
        for block in &batch {
            let actual = hex::encode(block_hash(block));
            if actual != block.hash || !expected.contains(&actual) {
                warn!(
                    peer = %addr,
                    got = %actual,
                    "Discarding block that does not match a requested header"
                );
                return Err(StrangecoinError::InvalidBlock(
                    "headers-first: block hash mismatch".to_string(),
                ));
            }
            seen.insert(actual);
        }
        if seen.len() != batch.len() || batch.len() > chunk.len() {
            return Err(StrangecoinError::InvalidBlock(
                "headers-first: duplicate blocks in response".to_string(),
            ));
        }
        blocks.extend(batch);
        if blocks.len() == plan.headers.len() {
            break;
        }
    }
    if blocks.len() != plan.headers.len() {
        return Err(StrangecoinError::InvalidBlock(format!(
            "headers-first: got {} of {} blocks",
            blocks.len(),
            plan.headers.len()
        )));
    }

    // Phase 4: assemble the candidate. The bodies still go through the full
    // validate_chain rules — but in the SyncEngine, which is the only
    // adopter now (ADR-0010); headers only decided *what* to fetch.
    let mut candidate: Vec<Block> = local_chain[..=plan.fork_index as usize].to_vec();
    candidate.extend(blocks);
    let blocks_downloaded = plan.headers.len();
    let peer_tip_height = plan.candidate_info.tip_height;
    Ok(SyncOutcome {
        candidate: Some(candidate),
        headers_ingested: cache.len(),
        blocks_downloaded,
        peer_tip_height,
    })
}

/// Bound one framing operation by [`SYNC_IO_TIMEOUT`] (ADR-0011: replaces
/// the per-stream `set_read_timeout`/`set_write_timeout` pair).
async fn with_io_timeout<T>(
    fut: impl std::future::Future<Output = Result<T, StrangecoinError>>,
) -> Result<T, StrangecoinError> {
    timeout(SYNC_IO_TIMEOUT, fut)
        .await
        .map_err(|_| timeout_error())?
}

fn timeout_error() -> StrangecoinError {
    StrangecoinError::IoError(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "sync I/O timeout",
    ))
}

fn shutdown_error() -> StrangecoinError {
    StrangecoinError::IoError(std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "sync aborted by shutdown",
    ))
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;
    use strangecoin_core::serialize::header_hash;

    fn target_hex(prefix_zeros: usize) -> String {
        // `prefix_zeros` leading zero *nibbles* (hex chars) → harder target.
        let mut s = "0".repeat(prefix_zeros);
        s.push_str(&"f".repeat(64 - prefix_zeros));
        s
    }

    /// Mine a header on top of `previous` with the given target: with a test
    /// target this is a bounded nonce search, not real difficulty.
    fn mine_header(
        index: u64,
        previous_hash: &str,
        timestamp: u64,
        target: &str,
        tx_root: [u8; 32],
    ) -> BlockHeader {
        let mut header = BlockHeader {
            index,
            timestamp,
            previous_hash: previous_hash.to_string(),
            hash: String::new(),
            nonce: 0,
            target: target.to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root,
        };
        loop {
            header.hash = hex::encode(header_hash(&header));
            if validate_header_pow(&header).is_ok() {
                return header;
            }
            header.nonce += 1;
            header.hash = String::new();
        }
    }

    fn block_of(header: &BlockHeader) -> Block {
        Block::from_header(header.clone(), Vec::new())
    }

    #[test]
    fn bad_pow_header_never_reaches_fork_choice() {
        let g = mine_header(0, &"0".repeat(64), 1, &target_hex(0), [0u8; 32]);
        let local = vec![block_of(&g)];
        let locals: HashSet<String> = local.iter().map(|b| b.hash.clone()).collect();

        // Peer appends a header with a *correct* declared hash but an
        // impossible target: only the PoW check can reject it, and it must —
        // together with everything chained after it.
        let mut cheat = BlockHeader {
            index: 1,
            timestamp: 2,
            previous_hash: g.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target: target_hex(64),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [1u8; 32],
        };
        cheat.hash = hex::encode(header_hash(&cheat));
        let followed = mine_header(2, &cheat.hash, 3, &target_hex(0), [2u8; 32]);

        let mut cache = HeaderCache::new();
        assert_eq!(cache.ingest(&[g, cheat, followed], &locals), 1);
        assert_eq!(cache.len(), 1, "only the valid prefix survives");
        assert!(
            plan_best_branch(&local, &cache).is_none(),
            "a rejected header must not influence tip selection"
        );
    }

    #[test]
    fn ingest_stops_at_first_invalid_and_keeps_valid_prefix() {
        let genesis = mine_header(0, &"0".repeat(64), 1, &target_hex(0), [0u8; 32]);
        let next = mine_header(1, &genesis.hash, 2, &target_hex(0), [1u8; 32]);
        // Bad header: declares someone else's parent.
        let broken = BlockHeader {
            previous_hash: "11".repeat(32),
            ..mine_header(2, &"0".repeat(64), 3, &target_hex(0), [2u8; 32])
        };
        // Bad header: impossible PoW.
        let bad_pow = BlockHeader {
            index: 3,
            target: target_hex(64),
            ..mine_header(3, &next.hash, 4, &target_hex(0), [3u8; 32])
        };
        let after_bad = mine_header(4, &"0".repeat(64), 5, &target_hex(0), [4u8; 32]);

        let mut cache = HeaderCache::new();
        let locals = HashSet::new();
        let accepted = cache.ingest(
            &[genesis.clone(), next.clone(), broken.clone(), bad_pow, after_bad],
            &locals,
        );
        assert_eq!(
            accepted, 2,
            "ingestion must stop at the first bad header"
        );
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&genesis.hash).is_some());
        assert!(cache.get(&next.hash).is_some());
        assert_eq!(cache.max_index(), Some(1));
        assert_eq!(broken.index, 2);
    }

    #[test]
    fn ingest_links_first_header_to_local_chain() {
        let genesis = mine_header(0, &"0".repeat(64), 1, &target_hex(0), [0u8; 32]);
        let first_local = mine_header(1, &genesis.hash, 2, &target_hex(0), [1u8; 32]);
        let local_hashes: HashSet<String> = [genesis.hash.clone(), first_local.hash.clone()]
            .into_iter()
            .collect();
        // A batch starting mid-chain whose parent is a local block.
        let joined = mine_header(2, &first_local.hash, 3, &target_hex(0), [2u8; 32]);

        let mut cache = HeaderCache::new();
        let accepted = cache.ingest(std::slice::from_ref(&joined), &local_hashes);
        assert_eq!(accepted, 1, "header linking to local chain must be kept");

        // Same header, unknown parent → dropped.
        let mut other = HeaderCache::new();
        let orphan = BlockHeader {
            previous_hash: "22".repeat(32),
            ..joined
        };
        assert_eq!(other.ingest(&[orphan], &local_hashes), 0);
    }

    #[test]
    fn plan_picks_higher_work_branch() {
        // Local chain: genesis + 1 block on an easy target.
        let easy = target_hex(0);
        let g = mine_header(0, &"0".repeat(64), 1, &easy, [0u8; 32]);
        let local1 = mine_header(1, &g.hash, 10, &easy, [1u8; 32]);
        let local_chain = vec![block_of(&g), block_of(&local1)];

        // Peer cache: genesis + 2 blocks on a *harder* target (4 leading zero
        // nibbles → 16x work per block) → total work beats local.
        let hard = target_hex(4);
        let peer1 = mine_header(1, &g.hash, 5, &hard, [9u8; 32]);
        let peer2 = mine_header(2, &peer1.hash, 6, &hard, [10u8; 32]);
        let mut cache = HeaderCache::new();
        let locals: HashSet<String> = local_chain.iter().map(|b| b.hash.clone()).collect();
        let accepted = cache.ingest(&[g.clone(), peer1, peer2], &locals);
        assert_eq!(accepted, 3);

        let plan = plan_best_branch(&local_chain, &cache)
            .expect("harder/longer branch must win fork choice");
        assert_eq!(plan.fork_index, 0, "branches diverge after genesis");
        assert_eq!(plan.headers.len(), 2);
        assert_eq!(plan.candidate_info.tip_height, 2);
    }

    #[test]
    fn plan_returns_none_when_local_is_best() {
        let easy = target_hex(0);
        let g = mine_header(0, &"0".repeat(64), 1, &easy, [0u8; 32]);
        let local1 = mine_header(1, &g.hash, 10, &easy, [1u8; 32]);
        let local2 = mine_header(2, &local1.hash, 11, &easy, [2u8; 32]);
        let local_chain = vec![block_of(&g), block_of(&local1), block_of(&local2)];

        // Peer only knows genesis + a shorter sibling branch.
        let peer1 = mine_header(1, &g.hash, 3, &easy, [5u8; 32]);
        let mut cache = HeaderCache::new();
        let locals: HashSet<String> = local_chain.iter().map(|b| b.hash.clone()).collect();
        assert_eq!(cache.ingest(&[g, peer1], &locals), 2);

        assert!(
            plan_best_branch(&local_chain, &cache).is_none(),
            "shorter equal-work branch must not trigger a download"
        );
    }

    #[test]
    fn plan_ignores_branch_with_foreign_genesis() {
        let easy = target_hex(0);
        let local_g = mine_header(0, &"0".repeat(64), 1, &easy, [0u8; 32]);
        let local1 = mine_header(1, &local_g.hash, 2, &easy, [1u8; 32]);
        let local_chain = vec![block_of(&local_g), block_of(&local1)];

        // Peer's index-0 header differs (different genesis → different hash).
        let mut foreign_g = mine_header(0, &"0".repeat(64), 1, &easy, [77u8; 32]);
        foreign_g.tx_root = [78u8; 32];
        foreign_g.hash = hex::encode(header_hash(&foreign_g));
        let mut cache = HeaderCache::new();
        let locals: HashSet<String> = local_chain.iter().map(|b| b.hash.clone()).collect();
        assert_eq!(cache.ingest(&[foreign_g], &locals), 1);

        assert!(
            plan_best_branch(&local_chain, &cache).is_none(),
            "foreign-genesis branch must be skipped"
        );
    }
}
