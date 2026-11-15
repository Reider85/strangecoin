//! S3-P13: snapshot sync e2e — export/import + commitment verification +
//! new-node catch-up (`docs/ADR/0026-snapshot-sync-format.md`).
//!
//! DoD row 4 (`prompt-stage3.md`): «Snapshot sync работает (new node без
//! full chain download)».
//!
//! КГ mapping:
//! 1. export/import + format per ADR-0026 — first test (round-trip) +
//!    storage-crate unit tests (`snapshot.rs`);
//! 2. 4-step verify negatives (forged/corrupt/wrong-network) — content
//!    tests (forged flips a balance **and recomputes the trailer** so
//!    step (a) passes and step (c) fires);
//! 3. e2e catch-up без full replay — second test (direct block
//!    application, prompt-sanctioned alternative to network sync);
//! 4. generation cadence `--every N` — CLI parser tests in
//!    `src/cli/snapshot.rs` (auto-gen = ops-cron deviation);
//! 5. wire GET_SNAPSHOT — Stage 4+ residual (module docs).
//!
//! **Deviations (FBP, fixed in the S3-P13 commit):** tip block section in
//! the body (ADR lists only accounts + storage — parent linkage needs the
//! full block); account body = raw state-CF bytes (S3-P07 alignment);
//! `chain_base_height` partial-chain meta (beyond literal ADR «meta tip =
//! height»); catch-up via direct `add_block` (headers-first for snapshot
//! nodes is Stage 4+ residual).

mod common;

use common::*;
use strangecoin::consensus::CHAIN_ID_REGTEST;
use strangecoin::storage::{
    export_snapshot, import_snapshot, verify_snapshot_file, Cf, SnapshotError,
};
use strangecoin::BlockchainFacade;
use strangecoin_core::state::sparse_merkle::SparseMerkleTrie;
use strangecoin_core::types::Account;
#[allow(unused_imports)]
use strangecoin_storage::StorageEngine;

/// Mine until `chain_len() == target_len` (grant + transfers). Returns
/// the facade, its temp dir, and the wallet secret so the caller can keep
/// mining past an intermediate export height. Recipe mirrors
/// `tests/pruning.rs` (251-block precedent).
fn mined_chain(
    tag: &str,
    target_len: usize,
) -> (BlockchainFacade, TestDir, String, secp256k1::SecretKey) {
    let dir = TestDir::new(tag);
    let bc = create_test_blockchain(dir.path());
    let (wallet, wallet_sk) = generate_keypair();
    bc.grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests");
    let (alice, _) = generate_keypair();
    while bc.chain_len() < target_len {
        create_and_mine_tx(&bc, &wallet, &alice, 7, &wallet_sk);
    }
    (bc, dir, wallet, wallet_sk)
}

/// Keep mining `count` more blocks with the same wallet (used to advance
/// A past an intermediate export height).
fn mine_more(bc: &BlockchainFacade, wallet: &str, wallet_sk: &secp256k1::SecretKey, count: usize) {
    let (alice, _) = generate_keypair();
    for _ in 0..count {
        create_and_mine_tx(bc, wallet, &alice, 7, wallet_sk);
    }
}

/// All accounts of a facade as `HashMap<String, Account>` for root
/// recomputation (the storage crate exposes the raw engine; the facade
/// keeps the decoded cache).
fn all_accounts(bc: &BlockchainFacade) -> std::collections::HashMap<String, Account> {
    let engine = bc.storage_engine();
    let mut out = std::collections::HashMap::new();
    for (key, value) in engine
        .iterate_prefix(Cf::State, b"")
        .expect("iterate state")
    {
        let addr = String::from_utf8(key).expect("utf-8 address");
        let account: Account = serde_json::from_slice(&value).expect("account json");
        out.insert(addr, account);
    }
    out
}

/// Recompute the blake3 trailer over `header ‖ body` so a mutated file
/// still passes step (a) and fails on content (steps b/c/d).
fn recompute_trailer(bytes: &mut [u8]) {
    let n = bytes.len();
    assert!(n > 117 + 32, "file too short");
    let (rest, _) = bytes.split_at_mut(n - 32);
    let checksum = blake3::hash(rest);
    bytes[n - 32..].copy_from_slice(checksum.as_bytes());
}

/// Positive path: A (200 blocks) exports at tip → B fresh imports →
/// state root and tip match without replaying blocks 0..200.
#[test]
fn export_import_restores_state_and_tip() {
    let (a, _dir_a, _wallet, _sk) = mined_chain("snap_roundtrip_a", 201); // heights 0..=200
    let snap_dir = TestDir::new("snap_roundtrip_file");
    let snap_path = snap_dir.path().join("tip.scsnap");

    let exported =
        export_snapshot(a.storage_engine().as_ref(), &snap_path, CHAIN_ID_REGTEST).expect("export");
    assert_eq!(exported.height, 200);

    // Fresh B: import through the raw engine first (opening a facade on
    // an empty dir would install genesis and make the DB non-fresh).
    let dir_b = TestDir::new("snap_roundtrip_b");
    {
        let engine = strangecoin_storage::RocksDbEngine::open(dir_b.path()).expect("open fresh B");
        let imported = import_snapshot(&engine, &snap_path, CHAIN_ID_REGTEST).expect("import");
        assert_eq!(imported.height, 200);
        assert_eq!(imported.state_root, exported.state_root);
    }
    let b = create_test_blockchain(dir_b.path());

    // state_root B (recomputed from B's accounts) == A's tip commitment.
    let root_b = SparseMerkleTrie::compute_root(&all_accounts(&b));
    let a_tip = a.tip().expect("A tip");
    assert_eq!(root_b, a_tip.state_root, "state root must match");
    assert_eq!(
        b.tip().expect("B tip").hash,
        a_tip.hash,
        "tip hash must match"
    );
    // B holds the MTP window (up to 11 blocks ending at the tip), not
    // the full 0..=200 chain — no full replay happened.
    assert!(
        b.chain_len() <= 11,
        "B must hold only the recent window, got {}",
        b.chain_len()
    );
    assert!(b.chain_len() >= 1);
}

/// Catch-up: A mines 100 → export → A mines 100 more (→200) → B imports
/// the height-100 snapshot → direct `add_block` of blocks 101..=200 →
/// tips converge. No full replay of 0..=100 on B.
#[test]
fn snapshot_node_catches_up_via_direct_blocks() {
    let (a, _dir_a, wallet, wallet_sk) = mined_chain("snap_catchup_a", 101); // heights 0..=100
    let snap_dir = TestDir::new("snap_catchup_file");
    let snap_path = snap_dir.path().join("h100.scsnap");
    export_snapshot(a.storage_engine().as_ref(), &snap_path, CHAIN_ID_REGTEST)
        .expect("export at 100");

    // A continues to height 200.
    mine_more(&a, &wallet, &wallet_sk, 100);
    assert_eq!(a.tip().expect("A tip").index, 200);

    // B: import at height 100, then apply the remaining blocks directly.
    let dir_b = TestDir::new("snap_catchup_b");
    {
        let engine = strangecoin_storage::RocksDbEngine::open(dir_b.path()).expect("open fresh B");
        import_snapshot(&engine, &snap_path, CHAIN_ID_REGTEST).expect("import at 100");
    }
    let b = create_test_blockchain(dir_b.path());
    assert!(
        b.chain_len() <= 11,
        "B starts from the recent window only, got {}",
        b.chain_len()
    );
    assert_eq!(b.tip().expect("tip").index, 100);

    let remaining: Vec<_> = a
        .chain_snapshot()
        .into_iter()
        .filter(|blk| blk.index > 100)
        .collect();
    assert_eq!(remaining.len(), 100, "A mined 100 blocks past the snapshot");
    for blk in remaining {
        let idx = blk.index;
        b.add_block(blk)
            .unwrap_or_else(|e| panic!("add_block at {idx}: {e}"));
    }

    assert_eq!(b.tip().expect("B tip").hash, a.tip().expect("A tip").hash);
    // B holds heights 90..=200 (MTP window at import + 100 new blocks).
    assert_eq!(b.chain_len(), 111, "B holds 90..=200");
    let root_b = SparseMerkleTrie::compute_root(&all_accounts(&b));
    assert_eq!(root_b, a.tip().expect("A tip").state_root);
}

/// Negative: flip a balance digit inside the body, recompute the trailer
/// so step (a) passes — step (c) state-root recomputation must reject.
#[test]
fn forged_body_is_rejected() {
    let (a, _dir_a, _wallet, _sk) = mined_chain("snap_forged", 5);
    let snap_dir = TestDir::new("snap_forged_file");
    let snap_path = snap_dir.path().join("f.scsnap");
    export_snapshot(a.storage_engine().as_ref(), &snap_path, CHAIN_ID_REGTEST).expect("export");

    let mut bytes = std::fs::read(&snap_path).expect("read");
    let needle = b"\"balance\":";
    let idx = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("balance field present");
    let value_start = idx + needle.len();
    let value_end = value_start
        + bytes[value_start..]
            .iter()
            .position(|&b| b == b'}' || b == b',')
            .expect("value end");
    let d = &mut bytes[value_end - 1];
    assert!(d.is_ascii_digit(), "expected digit");
    *d = if *d == b'9' { b'0' } else { *d + 1 };
    recompute_trailer(&mut bytes);
    std::fs::write(&snap_path, &bytes).expect("write");

    let err = verify_snapshot_file(&snap_path, CHAIN_ID_REGTEST).expect_err("must reject");
    assert!(
        matches!(err, SnapshotError::StateRootMismatch { .. }),
        "expected StateRootMismatch, got {err:?}"
    );
}

/// Negative: flip one trailer byte → step (a) checksum mismatch.
#[test]
fn corrupt_checksum_is_rejected() {
    let (a, _dir_a, _wallet, _sk) = mined_chain("snap_corrupt", 5);
    let snap_dir = TestDir::new("snap_corrupt_file");
    let snap_path = snap_dir.path().join("c.scsnap");
    export_snapshot(a.storage_engine().as_ref(), &snap_path, CHAIN_ID_REGTEST).expect("export");

    let mut bytes = std::fs::read(&snap_path).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&snap_path, &bytes).expect("write");

    let err = verify_snapshot_file(&snap_path, CHAIN_ID_REGTEST).expect_err("must reject");
    assert!(
        matches!(err, SnapshotError::ChecksumMismatch),
        "expected ChecksumMismatch, got {err:?}"
    );
}

/// Negative: file declares network 1, verifier expects regtest 3 →
/// step (b) rejects (header patched + trailer recomputed so (a) passes).
#[test]
fn wrong_network_id_is_rejected() {
    let (a, _dir_a, _wallet, _sk) = mined_chain("snap_netid", 5);
    let snap_dir = TestDir::new("snap_netid_file");
    let snap_path = snap_dir.path().join("n.scsnap");
    export_snapshot(a.storage_engine().as_ref(), &snap_path, CHAIN_ID_REGTEST).expect("export");

    let mut bytes = std::fs::read(&snap_path).expect("read");
    // network_id lives at header bytes [9..13] (u32 BE).
    bytes[9..13].copy_from_slice(&1u32.to_be_bytes());
    recompute_trailer(&mut bytes);
    std::fs::write(&snap_path, &bytes).expect("write");

    let err = verify_snapshot_file(&snap_path, CHAIN_ID_REGTEST).expect_err("must reject");
    assert!(
        matches!(
            err,
            SnapshotError::NetworkIdMismatch {
                found: 1,
                expected: 3
            }
        ),
        "expected NetworkIdMismatch, got {err:?}"
    );
}

/// Import is fail-closed on a DB that already carries a chain.
#[test]
fn import_requires_fresh_db() {
    let (a, _dir_a, _wallet, _sk) = mined_chain("snap_fresh_src", 5);
    let snap_dir = TestDir::new("snap_fresh_file");
    let snap_path = snap_dir.path().join("f.scsnap");
    export_snapshot(a.storage_engine().as_ref(), &snap_path, CHAIN_ID_REGTEST).expect("export");

    // B opens as a normal node first (installs genesis → tip meta exists).
    let dir_b = TestDir::new("snap_fresh_b");
    let b = create_test_blockchain(dir_b.path());
    assert!(b.chain_len() >= 1);
    // open_local does not save at open — persist genesis so tip meta
    // exists and import's freshness gate fires.
    b.save_state();
    drop(b);

    let engine = strangecoin_storage::RocksDbEngine::open(dir_b.path()).expect("reopen B");
    let err = import_snapshot(&engine, &snap_path, CHAIN_ID_REGTEST).expect_err("must refuse");
    assert!(
        matches!(err, SnapshotError::NotFreshDatabase),
        "expected NotFreshDatabase, got {err:?}"
    );
}
