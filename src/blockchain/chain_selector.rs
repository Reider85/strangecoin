//! # Chain selector (ARCHITECT3 §3.4, component 1)
//!
//! Fork choice for the node. The pure algorithm (work → earliest timestamp →
//! lowest hash) lives in `strangecoin_core::chain_selector` (0-I/O, proptest
//! coverage). This component is the node-level adapter:
//!
//! * re-exports [`ChainSelector`] / [`ChainInfo`];
//! * owns adoption against live [`Blockchain`] state
//!   ([`try_adopt_candidate`]);
//! * chain query helpers used by the facade wire path
//!   ([`headers_from_height`], [`blocks_by_hashes`]);
//! * wire snapshot shape + serde for `Blockchain`
//!   ([`BlockchainDeserialize`], moved from the facade in S1.5-P04 residual).
//!
//! BUG-S0-019: this file is no longer a 2-line re-export — adoption and wire
//! queries are first-class component responsibilities. The *algorithm* staying
//! in core is intentional (strangler pattern; ARCHITECT3 §3.2).
//!
//! BUG-S0-018 residual: serde/wire lives here so the facade stays ≤400 lines.

pub use strangecoin_core::chain_selector::{ChainInfo, ChainSelector};

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};
use strangecoin_core::types::{Block, BlockHeader, Transaction};
use tracing::warn;

use super::block_executor;
use super::blockchain_facade::Blockchain;
use super::consensus_manager::ConsensusManager;
use super::state_cache::StateCache;
use crate::error::StrangecoinError;
use crate::AccountState;

/// Headers for the HEADERS response: `index >= from_height`, ascending, at most `max`.
pub(crate) fn headers_from_height(
    chain: &[Block],
    from_height: u64,
    max: usize,
) -> Vec<BlockHeader> {
    chain
        .iter()
        .filter(|b| b.index >= from_height)
        .take(max)
        .map(|b| b.header())
        .collect()
}

/// Full blocks for the BLOCKS response: one block per requested hash, in order.
pub(crate) fn blocks_by_hashes(chain: &[Block], hashes: &[[u8; 32]], max: usize) -> Vec<Block> {
    if hashes.is_empty() {
        return Vec::new();
    }
    let mut by_hash: HashMap<&str, &Block> = HashMap::with_capacity(chain.len());
    for block in chain {
        by_hash.insert(&block.hash, block);
    }
    hashes
        .iter()
        .filter_map(|hash| by_hash.get(hex::encode(hash).as_str()))
        .take(max)
        .map(|b| (*b).clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Wire snapshot shape + serde for Blockchain (S1.5-P04 residual, BUG-S0-018)
// ---------------------------------------------------------------------------

/// Wire shape of a chain snapshot: what peers send and what `sync_rx` carries.
#[derive(Deserialize, Serialize)]
pub struct BlockchainDeserialize {
    pub chain: Vec<Block>,
    pub balances: HashMap<String, AccountState>,
    pub difficulty: u32,
    #[serde(default)]
    pub pending_transactions: Vec<Transaction>,
    #[serde(default)]
    pub mempool_txs: Vec<Transaction>,
    #[serde(default)]
    pub total_work: strangecoin_core::consensus::U256,
}

impl Serialize for Blockchain {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Blockchain", 5)?;
        state.serialize_field("chain", &self.chain)?;
        state.serialize_field("balances", &self.balances)?;
        state.serialize_field("difficulty", &self.difficulty)?;
        state.serialize_field("mempool_txs", &self.mempool.transactions())?;
        state.serialize_field("total_work", &self.total_work)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Blockchain {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let BlockchainDeserialize {
            chain,
            balances,
            difficulty,
            pending_transactions,
            mempool_txs,
            total_work,
        } = BlockchainDeserialize::deserialize(deserializer)?;

        // Legacy serde-JSON wire path (tests/diagnostics): the snapshot does
        // not carry a chain; the receiving node pins its own network when it
        // adopts via `try_adopt_candidate` (BUG-S1-004).
        let mut mempool =
            crate::mempool::Mempool::new(strangecoin_core::consensus::CHAIN_ID_REGTEST);
        for tx in pending_transactions.into_iter().chain(mempool_txs) {
            let account = AccountState::default();
            let _ = mempool.insert(tx, &account);
        }
        // S3-P07: no config reaches this residual wire-deserialize path —
        // open the default backend at the default path (deviation note;
        // no test/call site exercises this constructor today).
        let storage =
            crate::storage::Storage::open_rocks(&crate::config::StorageConfig::default().path)
                .map_err(serde::de::Error::custom)?;

        let codes = super::block_executor::codes_from_chain(
            &chain,
            strangecoin_core::consensus::CHAIN_ID_REGTEST,
        );

        Ok(Blockchain {
            chain,
            balances: StateCache::from_accounts(balances),
            difficulty,
            mempool,
            storage,
            allow_grant_blocks: false,
            allow_zero_state_root: false,
            total_work,
            rules: ConsensusManager::new(),
            chain_id: strangecoin_core::consensus::CHAIN_ID_REGTEST,
            codes,
            receipts: Vec::new(),
            // Empty snapshots: nothing was read from `storage`, so the
            // first save must write everything (never diff against state
            // the DB does not hold).
            persisted_accounts: HashMap::new(),
            persisted_contract_storage: strangecoin_core::state::ContractStorageMap::new(),
            persisted_mempool: HashMap::new(),
            pending_state_history: Vec::new(),
        })
    }
}

/// Fork-choice adoption: install `candidate_chain` iff it beats the current
/// chain and passes full validation.
///
/// On success: chain/balances/difficulty/total_work replaced (balances from
/// chain reconstruction, invariant #1), mempool merged. Returns `false` when
/// the candidate loses fork choice or is invalid.
pub(crate) fn try_adopt_candidate(
    current: &mut Blockchain,
    candidate_chain: Vec<Block>,
    candidate_balances: Option<HashMap<String, AccountState>>,
    candidate_mempool_txs: Vec<Transaction>,
    candidate_difficulty: u32,
) -> Result<bool, StrangecoinError> {
    if candidate_chain.len() <= 1 {
        return Ok(false);
    }
    let candidate_info = match ChainSelector::chain_info(&candidate_chain) {
        Some(info) => info,
        None => return Ok(false),
    };
    let should_adopt = match ChainSelector::chain_info(&current.chain) {
        None => true,
        Some(current_info) => ChainSelector::is_better(&candidate_info, &current_info),
    };
    if !should_adopt {
        return Ok(false);
    }

    // Recorded rebuild (S3-P07): besides the cache/receipts, adoption
    // collects per-height pre-apply account snapshots — `state_history`
    // material for every height this adoption persists.
    let outcome = match StateCache::rebuild_from_chain_recorded(
        &candidate_chain,
        block_executor::now_secs(),
        current.allow_grant_blocks,
        current.allow_zero_state_root,
        &current.rules,
        current.chain_id,
    ) {
        Ok(outcome) => outcome,
        Err(e) => {
            warn!(error = %e, "Candidate chain rejected: failed validation");
            return Ok(false);
        }
    };
    let rebuilt = outcome.cache;

    if let Some(wire_balances) = candidate_balances {
        if !wire_balances.is_empty() {
            let wire_nonzero: HashMap<String, u64> = wire_balances
                .iter()
                .filter(|(_, acc)| acc.balance() != 0)
                .map(|(addr, acc)| (addr.clone(), acc.balance()))
                .collect();
            if wire_nonzero != rebuilt.nonzero_balances() {
                warn!("Candidate balances disagree with chain reconstruction; rejecting");
                return Ok(false);
            }
        }
    }

    let local_mempool = current.mempool.transactions();
    // S3-P09: the candidate chain is not persisted yet, so the storage
    // txid index cannot see its txs — one HashSet pass over the candidate
    // replaces the old O(mempool × chain) `chain_has_tx` scan with O(1)
    // membership per mempool tx. (The facade-level `chain_contains_tx`
    // goes through the `txid → (height, index)` index instead.)
    let candidate_txids: HashSet<[u8; 32]> = candidate_chain
        .iter()
        .flat_map(|block| block.transactions.iter())
        .map(strangecoin_core::serialize::txid)
        .collect();
    let mut merged = crate::mempool::Mempool::new(current.chain_id);
    let mut merged_txids: Vec<[u8; 32]> = Vec::new();
    for tx in candidate_mempool_txs.into_iter().chain(local_mempool) {
        let txid = strangecoin_core::serialize::txid(&tx);
        if candidate_txids.contains(&txid) {
            continue;
        }
        if merged_txids.contains(&txid) {
            continue;
        }
        let account = rebuilt.get(tx.sender()).unwrap_or_default();
        if merged.insert(tx, &account).is_ok() {
            merged_txids.push(txid);
        }
    }

    current.chain = candidate_chain;
    current.balances = rebuilt;
    current.difficulty = candidate_difficulty;
    current.total_work = strangecoin_core::consensus::cumulative_work(&current.chain);
    current.mempool = merged;
    current.receipts = outcome.receipts;
    // Replace (not append): the candidate's history supersedes anything
    // an earlier unsaved commit might have staged; `save_state` writes
    // only the heights past the persisted common ancestor and clears the
    // rest.
    current.pending_state_history = outcome.history;
    current.codes = block_executor::codes_from_chain(&current.chain, current.chain_id);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_and_blocks_helpers() {
        let h64 = |s: char| s.to_string().repeat(64);
        let mk = |i: u64, hash: &str| Block {
            index: i,
            timestamp: i,
            transactions: vec![],
            previous_hash: h64('0'),
            hash: hash.to_string(),
            nonce: 0,
            target: "ff".repeat(32),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
            receipts_root: [0u8; 32],
        };
        let chain = vec![mk(0, &h64('a')), mk(1, &h64('b')), mk(2, &h64('c'))];
        let headers = headers_from_height(&chain, 1, 10);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].index, 1);

        let target = strangecoin_core::serialize::block_hash(&chain[2]);
        // Lookup uses the stored `hash` field, not a recomputed hash.
        let stored = chain[2].hash.clone();
        let _ = target;
        let blocks = blocks_by_hashes(
            &chain,
            &[hex::decode(&stored).unwrap().try_into().unwrap()],
            10,
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].index, 2);
    }
}
