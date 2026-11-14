//! S3-P09: O(1) indices written at adopt — `txid → (height, tx_index)`
//! plus the raw tx, `block hash → height`, `contract address →
//! storage_root` — consistent across reorgs, backfilled for pre-index
//! databases.
//!
//! КГ: lookups hit (positive + negative), reorg consistency
//! (`adopt_candidate` + revert), address→balance deviation (state CF is
//! already O(1), no separate index — see the S3-P09 commit message).

mod common;

use common::*;
use strangecoin::storage::{
    block_hash_index_key, decode_block_height_key, decode_tx_index_value, tx_index_key, tx_raw_key,
    Cf, RocksDbEngine, StorageEngine,
};
use strangecoin_core::serialize::txid;
use strangecoin_core::types::Transaction;

/// Assert the tx indices of `chain`.
///
/// Duplicate txids are a legal chain property here: regtest reward is 0
/// (`REGTEST_REWARD`) and the miner address comes from a stable key set,
/// so consecutive blocks can carry byte-identical coinbases (same txid).
/// The index stores one deterministic occurrence — the highest — so
/// coinbases assert "the entry resolves back to this txid and the raw
/// copy round-trips", while unique txs (transfers/creates/calls) assert
/// the exact `(height, index)`.
fn assert_tx_locations(bc: &strangecoin::BlockchainFacade, chain: &[strangecoin::Block]) {
    // Resolution goes against the facade's CURRENT full chain: an index
    // entry may legitimately point at a later occurrence of the same
    // duplicate txid than the sub-chain slice under assertion.
    let full = bc.chain_snapshot();
    for block in chain {
        for (index, tx) in block.transactions.iter().enumerate() {
            let id = txid(tx);
            let location = bc.tx_location(&id);
            if tx.is_coinbase() {
                let (height, at_index) = location.unwrap_or_else(|| {
                    panic!("coinbase at {}:{index} must be indexed", block.index)
                });
                let at = full
                    .get(height as usize)
                    .and_then(|indexed| indexed.transactions.get(at_index as usize))
                    .unwrap_or_else(|| {
                        panic!("index entry ({height},{at_index}) outside the chain")
                    });
                assert_eq!(txid(at), id, "index entry must resolve back to this txid");
            } else {
                assert_eq!(
                    location,
                    Some((block.index, index as u32)),
                    "unique tx at {}:{index} must resolve exactly",
                    block.index
                );
            }
            let raw = bc.get_tx(&id).expect("raw tx must be indexed");
            assert_eq!(txid(&raw), id, "raw copy must round-trip to the txid");
            assert!(bc.chain_contains_tx(tx), "chain_contains_tx via index");
        }
    }
}

/// Every confirmed tx in `chain` must resolve to its exact `(height,
/// index)` and back through the raw copy; every block through the
/// hash→height index; unknown ids/hashes must miss.
#[test]
fn indices_written_on_adopt_and_lookups_are_o1() {
    let dir = TestDir::new("indices_basic");
    let bc = create_test_blockchain(dir.path());
    let (wallet, wallet_sk) = generate_keypair();
    assert!(bc
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    let (alice, _) = generate_keypair();
    while bc.chain_len() < 30 {
        create_and_mine_tx(&bc, &wallet, &alice, 7, &wallet_sk);
    }

    let snapshot = bc.chain_snapshot();
    assert!(snapshot.len() >= 30);

    // Positive lookups: every tx of every block, O(1) each.
    assert_tx_locations(&bc, &snapshot);

    // Positive block lookups: hash → height → body, for every height.
    for block in &snapshot {
        let found = bc
            .block_by_hash(&block.hash)
            .unwrap_or_else(|| panic!("block {} must resolve by hash", block.hash));
        assert_eq!(found.index, block.index, "hash index must yield the height");
        assert_eq!(found.hash, block.hash);
        assert!(bc.chain_contains_block(&block.hash));
    }

    // Negative lookups (КГ).
    let missing_txid = [0xEEu8; 32];
    assert_eq!(bc.tx_location(&missing_txid), None);
    assert!(bc.get_tx(&missing_txid).is_none());
    let missing_hash = hex::encode([0xEEu8; 32]);
    assert!(bc.block_by_hash(&missing_hash).is_none());
    assert!(!bc.chain_contains_block(&missing_hash));
    assert!(!bc.chain_contains_block("not-even-hex"));
    let unconfirmed = Transaction::transfer("a", "b", 1, 1, 3);
    assert!(!bc.chain_contains_tx(&unconfirmed));

    // Raw-engine cross-check: the on-disk key/value bytes are exactly
    // what the schema prescribes (facade reads could in principle lie).
    drop(bc);
    let engine = RocksDbEngine::open(dir.path()).expect("reopen raw");
    let all_blocks = engine
        .iterate_prefix(Cf::Blocks, b"")
        .expect("blocks iterate");
    let heights: Vec<u64> = all_blocks
        .iter()
        .filter_map(|(key, _)| decode_block_height_key(key))
        .collect();
    assert_eq!(
        heights,
        (0..snapshot.len() as u64).collect::<Vec<_>>(),
        "height keys stay contiguous and ascending beside the index keys"
    );
    assert_eq!(
        all_blocks.len(),
        2 * snapshot.len(),
        "each block contributes one height record + one hash-index record"
    );
    for block in &snapshot {
        let hash: [u8; 32] = hex::decode(&block.hash)
            .expect("hash hex")
            .try_into()
            .expect("hash width");
        let value = engine
            .get(Cf::Blocks, &block_hash_index_key(&hash))
            .expect("hash index read")
            .expect("hash index must exist");
        let indexed_height = u64::from_be_bytes(value.as_slice().try_into().expect("u64 BE"));
        assert_eq!(indexed_height, block.index);

        for (index, tx) in block.transactions.iter().enumerate() {
            let id = txid(tx);
            let value = engine
                .get(Cf::Transactions, &tx_index_key(&id))
                .expect("tx index read")
                .expect("tx index must exist");
            let (indexed_height, indexed_pos) = decode_tx_index_value(&value)
                .expect("tx index value must decode to (height, index)");
            let at = snapshot[indexed_height as usize]
                .transactions
                .get(indexed_pos as usize)
                .expect("indexed position inside the chain");
            assert_eq!(txid(at), id, "index value must resolve back to this txid");
            if !tx.is_coinbase() {
                assert_eq!(
                    (indexed_height, indexed_pos),
                    (block.index, index as u32),
                    "unique tx index value bytes"
                );
            }
            let raw = engine
                .get(Cf::Transactions, &tx_raw_key(&id))
                .expect("tx raw read")
                .expect("tx raw must exist");
            assert_eq!(
                raw,
                strangecoin_core::serialize::serialize_transaction_signed(tx),
                "raw tx must be the canonical bytes"
            );
        }
    }
}

/// Reorg (S3-P09 КГ): `adopt_candidate` rewrites the fork tail — the
/// superseded fork's tx/block indices must disappear, the new fork's
/// must land at the right `(height, index)`, and the shared prefix must
/// stay untouched. Exercised in both directions (adopt side chain, then
/// adopt a third chain that supersedes it — the "revert").
#[test]
fn reorg_keeps_indices_consistent() {
    let dir = TestDir::new("indices_reorg");
    let source = create_test_blockchain(dir.path());
    let (wallet, wallet_sk) = generate_keypair();
    assert!(source
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    let (alice, _) = generate_keypair();
    // heights 0..=4: genesis, grant, three A-transfers.
    while source.chain_len() < 5 {
        create_and_mine_tx(&source, &wallet, &alice, 7, &wallet_sk);
    }
    let a_snapshot = source.chain_snapshot();
    let shared_prefix: Vec<_> = a_snapshot[..3].to_vec(); // heights 0..=2
                                                          // Only "real" transfers: a coinbase at the same height with the same
                                                          // miner/reward is byte-identical across forks and would legitimately
                                                          // re-index — it is not an evicted tx.
    let a_txids: Vec<[u8; 32]> = a_snapshot[3..]
        .iter()
        .flat_map(|b| b.transactions.iter())
        .filter(|tx| !tx.is_coinbase())
        .map(txid)
        .collect();
    assert!(!a_txids.is_empty(), "test premise: A-fork carries txs");
    let a_hashes: Vec<String> = a_snapshot[3..].iter().map(|b| b.hash.clone()).collect();

    // --- fork B: adopts the shared prefix, mines a longer chain -------
    let dir_b = TestDir::new("indices_reorg_side");
    let side = create_test_blockchain(dir_b.path());
    assert!(side
        .adopt_candidate(shared_prefix.clone(), None, vec![], source.difficulty())
        .expect("side must adopt the prefix"));
    let (bob, _) = generate_keypair();
    while side.chain_len() < 6 {
        create_and_mine_tx(&side, &wallet, &bob, 9, &wallet_sk);
    }
    let b_snapshot = side.chain_snapshot();
    let b_txids: Vec<[u8; 32]> = b_snapshot[3..]
        .iter()
        .flat_map(|b| b.transactions.iter())
        .filter(|tx| !tx.is_coinbase())
        .map(txid)
        .collect();
    assert!(!b_txids.is_empty(), "test premise: B-fork carries txs");

    // --- source reorgs onto B -----------------------------------------
    assert!(source
        .adopt_candidate(b_snapshot.clone(), None, vec![], side.difficulty())
        .expect("longer B chain must be adoptable"));
    assert_eq!(source.chain_len(), b_snapshot.len());
    assert_eq!(source.tip_hash(), b_snapshot.last().unwrap().hash);

    for id in &a_txids {
        assert_eq!(source.tx_location(id), None, "A-fork tx must be unindexed");
        assert!(source.get_tx(id).is_none(), "A-fork raw copy must go");
    }
    for hash in &a_hashes {
        assert!(
            source.block_by_hash(hash).is_none(),
            "A-fork block must not resolve"
        );
        assert!(!source.chain_contains_block(hash));
    }
    // B-fork txs indexed at their new homes (duplicate coinbase txids
    // tolerated per the helper's contract), blocks resolvable by hash.
    assert_tx_locations(&source, &b_snapshot);
    for block in &b_snapshot {
        let found = source
            .block_by_hash(&block.hash)
            .expect("B-fork block must resolve");
        assert_eq!(found.index, block.index);
    }
    // Shared prefix (heights 0..=2) survived the reorg untouched.
    assert_tx_locations(&source, &shared_prefix);
    for block in &shared_prefix {
        assert!(
            source.block_by_hash(&block.hash).is_some(),
            "shared-prefix block {} must stay resolvable",
            block.index
        );
    }

    // --- fork E: supersedes B (the "revert" direction) -----------------
    let dir_e = TestDir::new("indices_reorg_side2");
    let side2 = create_test_blockchain(dir_e.path());
    assert!(side2
        .adopt_candidate(shared_prefix.clone(), None, vec![], source.difficulty())
        .expect("side2 must adopt the prefix"));
    let (eve, _) = generate_keypair();
    while side2.chain_len() < 8 {
        create_and_mine_tx(&side2, &wallet, &eve, 11, &wallet_sk);
    }
    let e_snapshot = side2.chain_snapshot();
    assert!(source
        .adopt_candidate(e_snapshot.clone(), None, vec![], side2.difficulty())
        .expect("longer E chain must be adoptable"));
    assert_eq!(source.tip_hash(), e_snapshot.last().unwrap().hash);

    for id in &b_txids {
        assert_eq!(
            source.tx_location(id),
            None,
            "B-fork tx must be unindexed after the revert"
        );
        assert!(source.get_tx(id).is_none());
    }
    for block in &b_snapshot[3..] {
        assert!(
            source.block_by_hash(&block.hash).is_none(),
            "B-fork block must not resolve after the revert"
        );
    }
    assert_tx_locations(&source, &e_snapshot);
    for block in &e_snapshot {
        assert!(source.block_by_hash(&block.hash).is_some());
    }
    // The original A-fork txs stay gone (they were evicted in round 1).
    for id in &a_txids {
        assert_eq!(source.tx_location(id), None);
    }
    // And the shared prefix is still intact after two reorgs.
    assert_tx_locations(&source, &shared_prefix);
}

/// Storage-root cache: written when a contract's storage flushes at
/// adopt, mirrored from the consensus-derived `AccountState.storage_root`,
/// absent for EOAs, and invalidated (entry dropped) when the reorg evicts
/// the contract. The reopen doubles as the loader-skip regression — a
/// tagged root key that leaked into the `address ‖ slot` parse would
/// corrupt (or brick) the load.
///
/// The storage write happens in the **constructor** (deploy path): the
/// CREATE frame registers code inline with the derived address, while a
/// post-deploy `Call` would go through `collect_block_codes`, whose
/// nonce-derived registry address disagrees with `apply_create` for
/// mempool-valid txs (pre-existing mismatch — observed here, fixed
/// outside S3-P09; see the commit note).
#[cfg(feature = "vm-wasm")]
#[test]
fn contract_storage_root_cache_tracks_flushes() {
    use common::vm_helpers::{derive, wat};
    use strangecoin::storage::contract_storage_root_key;
    use strangecoin_core::vm::TxStatus;

    const KEY: [u8; 32] = [0x11u8; 32];
    let init_code = wat(
        &[(0, &KEY), (32, b"val!")],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "constructor") (param i32 i32) (result i32)
          (drop (call $w (i32.const 0) (i32.const 32) (i32.const 4)))
          (i32.const 0))
        (func (export "main") (result i32)
          (i32.const 1))"#,
    )
    .into_bytes();

    let dir = TestDir::new("indices_root");
    let bc = create_test_blockchain(dir.path());
    let (alice, alice_sk) = generate_keypair();
    assert!(bc
        .grant_initial_balance_to_first_wallet(&alice)
        .expect("grant must be enabled in tests"));

    // Deploy with a constructor budget: the constructor writes KEY ->
    // "val!" during the CREATE frame, so the storage flush (and the root
    // cache write) lands with the deploy block itself.
    let mut create = Transaction::create_with(
        &alice,
        init_code.clone(),
        vec![],
        vec![],
        100,
        200_000,
        bc.get_nonce(&alice) + 1,
        strangecoin::consensus::CHAIN_ID_REGTEST,
    );
    sign_transaction(&mut create, &alice_sk);
    let create_id = txid(&create);
    bc.apply_tx(create).expect("deploy accepted");
    mine_current(&bc);
    let contract = derive(&alice, 0);
    assert!(
        bc.get_account(&contract)
            .map(|account| account.is_contract())
            .unwrap_or(false),
        "deploy must install the contract account"
    );
    let deploy_receipt = bc
        .tip_receipts()
        .into_iter()
        .find(|receipt| receipt.txid == create_id)
        .expect("deploy receipt");
    assert_eq!(
        deploy_receipt.status,
        TxStatus::Success,
        "constructor must succeed"
    );
    assert!(
        deploy_receipt.gas_used > 0 && deploy_receipt.gas_used <= 200_000,
        "constructor metered within the budget, got {}",
        deploy_receipt.gas_used
    );

    let account = bc.get_account(&contract).expect("contract account");
    let expected_root = account
        .as_contract()
        .expect("contract account")
        .storage_root;
    assert_ne!(expected_root, [0u8; 32], "constructor must flush storage");
    assert_eq!(
        bc.contract_storage_root(&contract),
        Some(expected_root),
        "cache must mirror the consensus-derived root after the flush"
    );
    assert_eq!(
        bc.contract_storage_root(&alice),
        None,
        "EOAs have no storage-root cache entry"
    );
    assert_eq!(bc.contract_storage_root("sc1doesnotexist"), None);

    // Raw bytes + loader-skip regression: reopen through the facade and
    // prove the tagged root key did not leak into the slot parse.
    drop(bc);
    {
        let engine = RocksDbEngine::open(dir.path()).expect("reopen raw");
        let raw_root = engine
            .get(Cf::ContractStorage, &contract_storage_root_key(&contract))
            .expect("root read")
            .expect("root cache entry must exist on disk");
        assert_eq!(raw_root, expected_root.to_vec());
        let slot_key = strangecoin::storage::contract_storage_key(&contract, &KEY);
        assert!(
            engine
                .get(Cf::ContractStorage, &slot_key)
                .expect("slot read")
                .is_some(),
            "the flushed slot itself must be on disk"
        );
    }
    let bc = create_test_blockchain(dir.path());
    let slots = bc.state_snapshot().contract_storage().clone();
    assert_eq!(
        slots.len(),
        1,
        "loader must see exactly the contract's slots (root key skipped)"
    );
    assert!(slots.contains_key(&contract));
    assert_eq!(
        bc.contract_storage_root(&contract),
        Some(expected_root),
        "cache survives the reload"
    );

    // Invalidation: a longer side chain that never carried the deploy
    // evicts the contract — the cache entry must go with the account.
    let snapshot = bc.chain_snapshot();
    let prefix: Vec<_> = snapshot[..2].to_vec(); // genesis + grant, no deploy
    let side_dir = TestDir::new("indices_root_side");
    let side = create_test_blockchain(side_dir.path());
    assert!(side
        .adopt_candidate(prefix, None, vec![], bc.difficulty())
        .expect("side must adopt the prefix"));
    let (bob, _) = generate_keypair();
    while side.chain_len() < 4 {
        create_and_mine_tx(&side, &alice, &bob, 7, &alice_sk);
    }
    assert!(bc
        .adopt_candidate(side.chain_snapshot(), None, vec![], side.difficulty())
        .expect("longer side chain must be adoptable"));

    assert!(
        bc.get_account(&contract).is_none(),
        "the contract must be gone after the reorg"
    );
    assert_eq!(
        bc.contract_storage_root(&contract),
        None,
        "cache entry must be invalidated with the evicted contract"
    );
    assert_eq!(bc.tx_location(&create_id), None, "deploy tx evicted");
    assert!(bc.get_tx(&create_id).is_none());
    assert!(bc.block_by_hash(&snapshot[2].hash).is_none());
}

/// Backfill (S3-P09): a database whose index records are wiped — the
/// shape a pre-S3-P09 dev DB or a LevelDB-migrated DB has — rebuilds
/// every index on the next save instead of silently answering `None`
/// forever.
#[test]
fn backfills_indices_for_pre_index_databases() {
    let dir = TestDir::new("indices_backfill");
    let bc = create_test_blockchain(dir.path());
    let (wallet, wallet_sk) = generate_keypair();
    assert!(bc
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    let (alice, _) = generate_keypair();
    while bc.chain_len() < 6 {
        create_and_mine_tx(&bc, &wallet, &alice, 7, &wallet_sk);
    }
    let pre_snapshot = bc.chain_snapshot();
    drop(bc);

    // Simulate the pre-index database: every index record gone, meta
    // and primary records untouched.
    {
        let engine = RocksDbEngine::open(dir.path()).expect("reopen raw");
        let hash_keys: Vec<Vec<u8>> = engine
            .iterate_prefix(Cf::Blocks, b"")
            .expect("blocks iterate")
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| key.len() == 33)
            .collect();
        assert!(!hash_keys.is_empty(), "wipe target must exist");
        for key in &hash_keys {
            engine.delete(Cf::Blocks, key).expect("wipe hash index");
        }
        let tx_keys: Vec<Vec<u8>> = engine
            .iterate_prefix(Cf::Transactions, b"")
            .expect("tx iterate")
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert!(!tx_keys.is_empty(), "wipe target must exist");
        for key in &tx_keys {
            engine
                .delete(Cf::Transactions, key)
                .expect("wipe tx records");
        }
    }

    // Reopen: the indices are genuinely gone…
    let bc = create_test_blockchain(dir.path());
    let wiped_tx = txid(&pre_snapshot[3].transactions[0]);
    assert_eq!(
        bc.tx_location(&wiped_tx),
        None,
        "the wipe must have removed the index"
    );
    assert!(
        bc.block_by_hash(&pre_snapshot[3].hash).is_none(),
        "the wipe must have removed the hash index"
    );

    // …and the next save (a normal mine) backfills the whole chain.
    create_and_mine_tx(&bc, &wallet, &alice, 7, &wallet_sk);
    let snapshot = bc.chain_snapshot();
    assert_eq!(snapshot.len(), pre_snapshot.len() + 1);
    assert_tx_locations(&bc, &snapshot);
    for block in &snapshot {
        assert!(
            bc.block_by_hash(&block.hash).is_some(),
            "backfill must re-index the hash of height {}",
            block.index
        );
    }
}

/// Local benchmark smoke (КГ: «benchmark-смoke … не assert-time в CI,
/// только local note») — run manually with `cargo test --test indices
/// -- --ignored`.10k point lookups on a warm cache.
#[test]
#[ignore = "local benchmark smoke — not asserted in CI"]
fn bench_10k_tx_lookups() {
    let dir = TestDir::new("indices_bench");
    let bc = create_test_blockchain(dir.path());
    let (wallet, wallet_sk) = generate_keypair();
    assert!(bc
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    let (alice, _) = generate_keypair();
    while bc.chain_len() < 50 {
        create_and_mine_tx(&bc, &wallet, &alice, 7, &wallet_sk);
    }
    let ids: Vec<[u8; 32]> = bc
        .chain_snapshot()
        .iter()
        .flat_map(|b| b.transactions.iter())
        .map(txid)
        .collect();
    assert!(!ids.is_empty());

    let start = std::time::Instant::now();
    let mut hits = 0usize;
    for i in 0..10_000 {
        if bc.tx_location(&ids[i % ids.len()]).is_some() {
            hits += 1;
        }
    }
    let elapsed = start.elapsed();
    println!("10k tx_location lookups: {elapsed:?} ({hits} hits)");
    assert_eq!(hits, 10_000, "every lookup must hit");
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "10k point lookups took {elapsed:?} — expected sub-ms each on a desktop"
    );
}
