//! # State cache (ARCHITECT3 §3.4, component 3)
//!
//! The single place where account balances and nonces are read. The cache is
//! always derived from the chain and can be brought back in line with it:
//!
//! * `commit` — install the state produced by [`block_executor`] for an
//!   appended block;
//! * `unapply_block` — roll the tip back when a reorg drops it;
//! * `invalidate` — drop everything when a completely different chain is
//!   installed wholesale;
//! * `rebuild_from_chain` — recompute the whole cache from the chain, using
//!   the same block-by-block validation `validate_chain` used to do inline.
//!
//! Invariant #1 («вся валидность — из цепочки»): when the cached accounts and
//! a reconstruction from the chain disagree, the reconstruction wins. The
//! test `rebuild_from_chain_repairs_tampered_cache` in `tests/state_cache.rs`
//! pins that down.

use std::collections::HashMap;

use serde::{Serialize, Serializer};
use strangecoin_core::state::{unapply_block, State};
use strangecoin_core::types::Block;

use crate::blockchain::block_executor::{self, BlockView};
use crate::error::StrangecoinError;
use crate::AccountState;

/// Balances/nonces cache. Serialized as a plain address → account map so the
/// LevelDB and peer-to-peer wire format stays byte-compatible with the
/// `HashMap` this field used to be.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateCache {
    accounts: HashMap<String, AccountState>,
}

impl StateCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_accounts(accounts: HashMap<String, AccountState>) -> Self {
        Self { accounts }
    }

    pub fn from_state(state: &State) -> Self {
        Self {
            accounts: state.balances.clone(),
        }
    }

    /// Snapshot of the cache in the form [`strangecoin_core::state`] works with.
    pub fn to_state(&self) -> State {
        State {
            balances: self.accounts.clone(),
        }
    }

    // ---------------------------------------------------------------- reads

    pub fn get(&self, address: &str) -> Option<AccountState> {
        self.accounts.get(address).cloned()
    }

    pub fn balance(&self, address: &str) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.balance)
            .unwrap_or(0)
    }

    pub fn nonce(&self, address: &str) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.nonce)
            .unwrap_or(0)
    }

    pub fn contains_key(&self, address: &str) -> bool {
        self.accounts.contains_key(address)
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.accounts.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &AccountState> {
        self.accounts.values()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &AccountState)> {
        self.accounts.iter()
    }

    /// Direct view of the underlying map — read-only escape hatch for
    /// serialization and bulk clones.
    pub fn accounts(&self) -> &HashMap<String, AccountState> {
        &self.accounts
    }

    pub fn total_supply(&self) -> u64 {
        self.accounts.values().map(|account| account.balance).sum()
    }

    /// Balances with a non-zero remainder, keyed by address: the form both
    /// sides of the cache-vs-reconstruction comparison use.
    pub fn nonzero_balances(&self) -> HashMap<String, u64> {
        self.accounts
            .iter()
            .filter(|(_, account)| account.balance != 0)
            .map(|(address, account)| (address.clone(), account.balance))
            .collect()
    }

    // --------------------------------------------------------------- writes

    /// Install the state produced by `block_executor::validate_and_apply`.
    pub fn commit(&mut self, state: State) {
        self.accounts = state.balances;
    }

    /// Replace the whole cache (used when another chain is adopted).
    pub fn replace(&mut self, other: StateCache) {
        self.accounts = other.accounts;
    }

    /// Drop every account: the caller is about to install a different chain
    /// and must not keep reading balances derived from the old one.
    pub fn invalidate(&mut self) {
        self.accounts.clear();
    }

    /// Reorg invalidation: roll the cache back by one block so it matches the
    /// chain with that block removed.
    pub fn unapply_block(&mut self, block: &Block) -> Result<(), StrangecoinError> {
        let rolled_back = unapply_block(&self.to_state(), block)?;
        self.commit(rolled_back);
        Ok(())
    }

    /// Credit an address directly (genesis allocation only — every other
    /// balance change must go through `block_executor`).
    pub fn credit(&mut self, address: &str, amount: u64) {
        self.accounts
            .entry(address.to_string())
            .or_default()
            .balance += amount;
    }

    /// Register an address with a zero balance/nonce so it is visible to
    /// `contains_key` (used when a fresh wallet needs an account entry).
    pub fn ensure_account(&mut self, address: &str) {
        self.accounts.entry(address.to_string()).or_default();
    }

    /// Rebuild the cache from the chain, validating every block on the way.
    ///
    /// This is the mechanism `validate_chain` used to run inline: whatever the
    /// cache says, this result is what the chain actually implies.
    /// `rules` supplies the consensus version per height (consensus_manager is
    /// the single source of rules — ARCHITECT3 §3.4).
    pub fn rebuild_from_chain(
        chain: &[Block],
        now: u64,
        allow_grant_blocks: bool,
        rules: &super::consensus_manager::ConsensusManager,
    ) -> Result<Self, StrangecoinError> {
        let mut state = State::new();
        for (height, block) in chain.iter().enumerate() {
            let view = BlockView::new(
                &chain[..height],
                now,
                allow_grant_blocks,
                rules.expected_version(height as u64),
            )
            .with_phase(rules.phase_at(height as u64));
            state = block_executor::validate_and_apply(&state, block, &view).map_err(|e| {
                tracing::warn!(block_index = height, error = %e, "Rebuild stopped at an invalid block");
                e
            })?;
        }
        Ok(Self::from_state(&state))
    }
}

impl Serialize for StateCache {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.accounts.serialize(serializer)
    }
}

impl<'a> IntoIterator for &'a StateCache {
    type Item = (&'a String, &'a AccountState);
    type IntoIter = std::collections::hash_map::Iter<'a, String, AccountState>;

    fn into_iter(self) -> Self::IntoIter {
        self.accounts.iter()
    }
}
