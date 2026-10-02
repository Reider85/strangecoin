//! Component tests for `state_cache`: rebuild-from-chain, reorg invalidation
//! and invariant #1 — when the cache disagrees with a reconstruction from the
//! chain, the reconstruction wins (S1-P12).

mod common;

use common::*;
use strangecoin::blockchain::block_executor::now_secs;
use strangecoin::blockchain::state_cache::StateCache;

fn rebuild(chain: &[strangecoin::Block]) -> StateCache {
    StateCache::rebuild_from_chain(chain, now_secs(), true).expect("chain must rebuild")
}

#[test]
fn rebuild_from_chain_reproduces_the_chain_state() {
    let _dir = TestDir::new("state_cache_rebuild");
    let bc = create_test_blockchain(_dir.path());

    let rebuilt = rebuild(&bc.chain);
    assert_eq!(rebuilt.nonzero_balances(), bc.balances.nonzero_balances());
    assert_eq!(rebuilt.balance("initial_wallet_address"), 10_000);
}

#[test]
fn rebuild_from_chain_repairs_a_tampered_cache() {
    let _dir = TestDir::new("state_cache_invariant");
    let mut bc = create_test_blockchain(_dir.path());
    let honest = rebuild(&bc.chain);

    // Tamper with the cache: an extra account and an inflated balance.
    bc.balances.credit("mallory", 999_999);
    bc.balances.credit("initial_wallet_address", 1);
    assert_ne!(bc.balances.nonzero_balances(), honest.nonzero_balances());

    // Invariant #1: the chain wins — the cache is recomputed from it.
    bc.rebuild_state_cache().expect("chain must rebuild");
    assert!(bc.balances.get("mallory").is_none(), "tampered account must be gone");
    assert_eq!(bc.balances.balance("initial_wallet_address"), 10_000);
    assert_eq!(bc.balances.nonzero_balances(), honest.nonzero_balances());
}

#[test]
fn unapply_block_invalidates_the_cache_on_reorg() {
    let _dir = TestDir::new("state_cache_unapply");
    let mut bc = create_test_blockchain(_dir.path());

    // A second block (the primary-issuance grant) so there is a tip to drop.
    let (wallet, _secret_key) = generate_keypair();
    assert!(bc
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    assert_eq!(bc.chain.len(), 2, "grant block must extend the chain");

    let tip = bc.chain.last().expect("chain has a tip").clone();
    let with_tip = rebuild(&bc.chain);
    assert_eq!(with_tip.balance(&wallet), 10_000);

    // Reorg away the tip: the cache must end up exactly where the shorter
    // chain says it should be, without re-walking the whole chain.
    let mut rolled_back = with_tip.clone();
    rolled_back.unapply_block(&tip).expect("tip must unapply");

    let without_tip = rebuild(&bc.chain[..bc.chain.len() - 1]);
    assert_eq!(rolled_back, without_tip);
    assert!(rolled_back.get(&wallet).is_none());
}

#[test]
fn invalidate_drops_every_account() {
    let _dir = TestDir::new("state_cache_invalidate");
    let mut bc = create_test_blockchain(_dir.path());
    assert!(!bc.balances.is_empty());

    bc.balances.invalidate();
    assert!(bc.balances.is_empty());
    assert_eq!(bc.balances.balance("initial_wallet_address"), 0);
}

#[test]
fn rebuild_from_chain_rejects_a_tampered_block() {
    let _dir = TestDir::new("state_cache_tampered_chain");
    let bc = create_test_blockchain(_dir.path());

    // Tamper with the payload but leave the stored header hash alone.
    let mut chain = bc.chain.clone();
    chain[0].tx_root = [0xff; 32];
    let err = StateCache::rebuild_from_chain(&chain, now_secs(), true)
        .expect_err("a tampered block must not rebuild");
    assert!(format!("{err}").contains("block 0"), "{err}");
    assert!(format!("{err}").contains("header hash mismatch"), "{err}");

    // Tamper again and this time keep the header self-consistent: only the
    // transaction commitment gives the change away.
    let mut chain = bc.chain.clone();
    chain[0].tx_root = [0xff; 32];
    chain[0].hash = hex::encode(strangecoin_core::serialize::block_hash(&chain[0]));
    let err = StateCache::rebuild_from_chain(&chain, now_secs(), true)
        .expect_err("a recomputed header over a bad tx_root must not rebuild");
    assert!(format!("{err}").contains("tx root mismatch"), "{err}");
}
