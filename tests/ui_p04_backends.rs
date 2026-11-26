//! UI-P04 — in-process dashboard / transaction-audit adapters (ADR-0031
//! Amendment D). These tests prove the new screen seams read real node state
//! and fail closed on unknown txids, without any mock data in the adapter.

mod common;

use std::sync::Arc;

use common::*;
use strangecoin::api::{InProcessDashboardBackend, InProcessTxDetailBackend};
use strangecoin::consensus::CHAIN_ID_REGTEST;
use strangecoin::events::NodeEvent;
use strangecoin::wallet::Wallet;
use strangecoin_core::serialize::txid;
use strangecoin_net::{NodeKeypair, PeerManager, PeerManagerConfig};
use strangecoin_ui::{DashboardBackend, TxDetailBackend};

const PASSWORD: &str = "ui-p04-backend-password";

#[test]
fn dashboard_backend_reads_real_chain_state() {
    let dir = TestDir::new("ui_p04_dashboard");
    let wallet = Wallet::new(PASSWORD, dir.path()).expect("wallet");
    let address = strangecoin::address::encode_address(&wallet.public_key, CHAIN_ID_REGTEST)
        .expect("bech32 address");
    let chain = Arc::new(create_test_blockchain(dir.path()));
    chain
        .grant_initial_balance_to_first_wallet(&address)
        .expect("grant");
    chain.save_state();

    let expected_tip_block = chain.tip().expect("tip");
    let expected_height = expected_tip_block.index;
    let expected_tip = expected_tip_block.hash.clone();
    let peers = Arc::new(PeerManager::new(
        PeerManagerConfig::default(),
        NodeKeypair::from_seed([0xA1; 32]),
    ));
    let backend = InProcessDashboardBackend::new(Arc::clone(&chain), peers);

    let snapshot = backend.snapshot();
    assert_eq!(snapshot.sync.current_height, expected_height);
    assert_eq!(snapshot.sync.tip_height, expected_height);
    assert_eq!(snapshot.sync.sync_percent, 100);
    assert!(!snapshot.blocks.is_empty());
    assert_eq!(snapshot.blocks[0].hash, expected_tip);
    assert!(snapshot.peers.is_empty(), "fresh PeerManager has no peers");
    assert!(snapshot.mempool.is_empty());

    backend.record_event(&NodeEvent::BlockApplied {
        height: expected_height,
        hash: expected_tip,
    });
    let digest = backend.digest_handle();
    let digest = digest.lock().expect("digest lock");
    assert_eq!(digest.blocks_applied, 1);
    assert_eq!(digest.last_height, expected_height);
}

#[test]
fn tx_detail_backend_loads_real_tx_and_keeps_witness_lazy() {
    let dir = TestDir::new("ui_p04_tx_detail");
    let wallet = Wallet::new(PASSWORD, dir.path()).expect("wallet");
    let address = strangecoin::address::encode_address(&wallet.public_key, CHAIN_ID_REGTEST)
        .expect("bech32 address");
    let chain = Arc::new(create_test_blockchain(dir.path()));
    chain
        .grant_initial_balance_to_first_wallet(&address)
        .expect("grant");
    chain.save_state();

    let chain_view = chain.chain_snapshot();
    let (height, known) = chain_view
        .iter()
        .find_map(|block| block.transactions.first().map(|tx| (block.index, tx)))
        .expect("grant block must contain a tx");
    let known_id = format!("0x{}", hex::encode(txid(known)));

    let backend = InProcessTxDetailBackend::new(chain);
    let detail = backend.load(&known_id).expect("known txid must load");
    let overview = detail.overview.expect("overview");
    assert_eq!(overview.txid, known_id);
    assert_eq!(overview.block_height, Some(height));
    assert!(detail.receipt.is_some());
    assert!(detail.witness.is_none(), "witness must only load on Verify");
    assert!(backend.load("0x00").is_err(), "unknown txid fails closed");
}
