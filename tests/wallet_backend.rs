//! UI-P03 — in-process wallet backend e2e (ADR-0031 Amendment C).
//!
//! The professional wallet screen has no direct node access; it talks to
//! `WalletBackend`. These tests prove the in-process adapter over
//! `BlockchainFacade` really reads chain state, signs/admit transfers and
//! fails closed for the not-yet-implemented batch seam.

mod common;

use std::sync::Arc;

use common::*;
use strangecoin::api::InProcessWalletBackend;
use strangecoin::consensus::CHAIN_ID_REGTEST;
use strangecoin::wallet::Wallet;
use strangecoin_ui::{BatchCall, WalletBackend};

const PASSWORD: &str = "ui-p03-backend-password";

struct Fixture {
    _dir: TestDir,
    backend: InProcessWalletBackend,
    address: String,
    recipient: String,
}

fn fixture() -> Fixture {
    let dir = TestDir::new("wallet_backend");
    let wallet = Wallet::new(PASSWORD, dir.path()).expect("Wallet::new must succeed");
    let address = strangecoin::address::encode_address(&wallet.public_key, CHAIN_ID_REGTEST)
        .expect("bech32 address");

    let keypairs = generate_keypairs(2);
    let recipient = keypairs[1].0.clone();

    let chain = Arc::new(create_test_blockchain(dir.path()));
    chain
        .grant_initial_balance_to_first_wallet(&address)
        .expect("grant must succeed");
    // `grant_initial_balance_to_first_wallet` mutates the in-memory chain;
    // the GUI registration handler persists it the same way.
    chain.save_state();

    let backend = InProcessWalletBackend::new(
        Arc::clone(&chain),
        CHAIN_ID_REGTEST,
        dir.path().to_path_buf(),
    );
    {
        let session = backend.session();
        let mut session = session.lock().unwrap();
        session.address = address.clone();
        session.password = PASSWORD.to_string();
    }

    Fixture {
        _dir: dir,
        backend,
        address,
        recipient,
    }
}

#[test]
fn backend_reads_balance_and_history_from_chain() {
    let fixture = fixture();
    assert!(fixture.backend.validate_address(&fixture.address));
    assert!(!fixture.backend.validate_address("not-a-bech32-address"));
    assert_eq!(
        fixture.backend.get_balance(&fixture.address).unwrap(),
        10000
    );

    let history = fixture
        .backend
        .recent_transactions(&fixture.address, 50)
        .unwrap();
    assert!(!history.is_empty(), "grant block must appear in history");
    assert!(history
        .iter()
        .all(|tx| tx.height.is_some() && tx.txid.starts_with("0x")));

    let known = &history[0].txid;
    assert!(
        fixture.backend.get_transaction(known).unwrap().is_some(),
        "eth_getTransactionByHash seam must resolve a known txid"
    );
}

#[test]
fn backend_signs_and_submits_transfer() {
    let fixture = fixture();
    let txid = fixture
        .backend
        .submit_transaction(&fixture.recipient, 1000, 0)
        .expect("signed transfer must be admitted");
    assert_eq!(txid.len(), 66, "0x + 32-byte hex hash");

    let history = fixture
        .backend
        .recent_transactions(&fixture.address, 50)
        .unwrap();
    assert!(
        history
            .iter()
            .any(|tx| tx.txid == txid && tx.height.is_none()),
        "submitted tx must appear as pending in history"
    );
}

#[test]
fn backend_fails_closed_for_unavailable_features() {
    let fixture = fixture();
    assert_eq!(fixture.backend.estimate_fee(), 0);
    let err = fixture.backend.submit_batch(&[]).unwrap_err();
    assert!(
        err.to_string().contains("AI-P04"),
        "batch must honestly report the AI-P04 gate, got: {err}"
    );
    assert!(fixture
        .backend
        .submit_transaction("bad-address", 1, 0)
        .is_err());

    let calls = vec![BatchCall {
        to: fixture.recipient.clone(),
        amount: 1,
        calldata: Vec::new(),
    }];
    assert!(fixture.backend.submit_batch(&calls).is_err());
}
