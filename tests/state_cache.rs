//! Component tests for `state_cache`: rebuild-from-chain, reorg invalidation
//! and invariant #1 — when the cache disagrees with a reconstruction from the
//! chain, the reconstruction wins (S1-P12).

mod common;

use common::*;
use strangecoin::blockchain::block_executor::now_secs;
use strangecoin::blockchain::state_cache::StateCache;
use strangecoin::ConsensusManager;

fn rebuild(chain: &[strangecoin::Block]) -> StateCache {
    StateCache::rebuild_from_chain(chain, now_secs(), true, &ConsensusManager::new())
        .expect("chain must rebuild")
}

#[test]
fn rebuild_from_chain_reproduces_the_chain_state() {
    let _dir = TestDir::new("state_cache_rebuild");
    let bc = create_test_blockchain(_dir.path());

    let chain = bc.chain_snapshot();
    let rebuilt = rebuild(&chain);
    assert_eq!(rebuilt.nonzero_balances(), bc.nonzero_balances());
    assert_eq!(rebuilt.balance("initial_wallet_address"), 10_000);
}

#[test]
fn rebuild_from_chain_repairs_a_tampered_cache() {
    let _dir = TestDir::new("state_cache_invariant");
    let bc = create_test_blockchain(_dir.path());
    let chain = bc.chain_snapshot();
    let honest = rebuild(&chain);

    // Tamper with the cache: an extra account and an inflated balance.
    bc.with_state_cache_mut(|sc| {
        sc.credit("mallory", 999_999);
        sc.credit("initial_wallet_address", 1);
    });
    assert_ne!(bc.nonzero_balances(), honest.nonzero_balances());

    // Invariant #1: the chain wins — the cache is recomputed from it.
    bc.rebuild_state_cache().expect("chain must rebuild");
    assert!(
        bc.get_account("mallory").is_none(),
        "tampered account must be gone"
    );
    assert_eq!(bc.get_balance("initial_wallet_address"), 10_000);
    assert_eq!(bc.nonzero_balances(), honest.nonzero_balances());
}

#[test]
fn unapply_block_invalidates_the_cache_on_reorg() {
    let _dir = TestDir::new("state_cache_unapply");
    let bc = create_test_blockchain(_dir.path());

    // A second block (the primary-issuance grant) so there is a tip to drop.
    let (wallet, _secret_key) = generate_keypair();
    assert!(bc
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    assert_eq!(bc.chain_len(), 2, "grant block must extend the chain");

    let tip = bc.tip().expect("chain has a tip");
    let chain = bc.chain_snapshot();
    let with_tip = rebuild(&chain);
    assert_eq!(with_tip.balance(&wallet), 10_000);

    // Reorg away the tip: the cache must end up exactly where the shorter
    // chain says it should be, without re-walking the whole chain.
    let mut rolled_back = with_tip.clone();
    rolled_back.unapply_block(&tip).expect("tip must unapply");

    let without_tip = rebuild(&chain[..chain.len() - 1]);
    assert_eq!(rolled_back, without_tip);
    assert!(rolled_back.get(&wallet).is_none());
}

#[test]
fn invalidate_drops_every_account() {
    let _dir = TestDir::new("state_cache_invalidate");
    let bc = create_test_blockchain(_dir.path());
    assert!(!bc.state_snapshot().is_empty());

    bc.with_state_cache_mut(|sc| sc.invalidate());
    assert!(bc.state_snapshot().is_empty());
    assert_eq!(bc.get_balance("initial_wallet_address"), 0);
}

#[test]
fn rebuild_from_chain_rejects_a_tampered_block() {
    let _dir = TestDir::new("state_cache_tampered_chain");
    let bc = create_test_blockchain(_dir.path());

    // Tamper with the payload but leave the stored header hash alone.
    let mut chain = bc.chain_snapshot();
    chain[0].tx_root = [0xff; 32];
    let err = StateCache::rebuild_from_chain(&chain, now_secs(), true, &ConsensusManager::new())
        .expect_err("a tampered block must not rebuild");
    assert!(format!("{err}").contains("block 0"), "{err}");
    assert!(format!("{err}").contains("header hash mismatch"), "{err}");

    // Tamper again and this time keep the header self-consistent: only the
    // transaction commitment gives the change away.
    let mut chain = bc.chain_snapshot();
    chain[0].tx_root = [0xff; 32];
    chain[0].hash = hex::encode(strangecoin_core::serialize::block_hash(&chain[0]));
    let err = StateCache::rebuild_from_chain(&chain, now_secs(), true, &ConsensusManager::new())
        .expect_err("a recomputed header over a bad tx_root must not rebuild");
    assert!(format!("{err}").contains("tx root mismatch"), "{err}");
}
