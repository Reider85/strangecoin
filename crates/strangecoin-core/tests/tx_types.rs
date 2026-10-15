//! S1.5-P08: Tx types — transfer / create / call.
//!
//! Covers the per-variant canonical encoding, signature verification,
//! shape validation, serde compatibility and the apply_block gate that
//! defers Create/Call execution to S1.5-P15.

use secp256k1::{ecdsa::RecoverableSignature, Message, Secp256k1, SecretKey};
use strangecoin_core::consensus::{
    verify_transaction, CHAIN_ID_MAINNET, CHAIN_ID_REGTEST,
};
use strangecoin_core::error::CoreError;
use strangecoin_core::serialize::{
    deserialize_transaction, deserialize_transaction_signed, serialize_transaction,
    serialize_transaction_signed, txid,
};
use strangecoin_core::state::{apply_block, unapply_block, State};
use strangecoin_core::types::{Block, CallTx, CreateTx, Transaction, TransferTx};

fn keypair() -> (String, SecretKey) {
    let secp = Secp256k1::new();
    let mut sk_bytes = [7u8; 32];
    sk_bytes[0] = 42;
    let sk = SecretKey::from_slice(&sk_bytes).expect("valid secret key");
    let pk = secp256k1::PublicKey::from_secret_key(&secp, &sk);
    let addr = strangecoin_core::address::encode_address(&pk, CHAIN_ID_REGTEST)
        .expect("encode address");
    (addr, sk)
}

fn sign(tx: &mut Transaction, sk: &SecretKey) {
    let secp = Secp256k1::new();
    let msg_bytes = serialize_transaction(tx);
    let msg_hash = blake3::hash(&msg_bytes);
    let msg = Message::from_digest_slice(msg_hash.as_bytes()).expect("message digest");
    let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, sk);
    let (rec_id, sig_bytes) = sig.serialize_compact();
    let mut sig_vec = Vec::with_capacity(65);
    sig_vec.extend_from_slice(&sig_bytes);
    sig_vec.push(rec_id.to_i32() as u8);
    *tx.signature_mut() = sig_vec;
}

// ---------------------------------------------------------------------------
// Round-trip serialization
// ---------------------------------------------------------------------------

#[test]
fn transfer_roundtrip_unsigned_signed_txid() {
    let tx = Transaction::transfer("alice", "bob", 100, 1, CHAIN_ID_REGTEST);
    let unsigned = serialize_transaction(&tx);
    assert_eq!(deserialize_transaction(&unsigned).unwrap(), tx);

    let mut signed = tx.clone();
    *signed.signature_mut() = vec![1, 2, 3];
    let bytes = serialize_transaction_signed(&signed);
    assert_eq!(deserialize_transaction_signed(&bytes).unwrap(), signed);

    assert_eq!(txid(&tx), txid(&tx));
    assert_ne!(txid(&tx), [0u8; 32]);
}

#[test]
fn create_roundtrip() {
    let tx = Transaction::create("alice", vec![0x00, 0x61], vec![9, 9, 9], 50, 3, CHAIN_ID_MAINNET);
    let unsigned = serialize_transaction(&tx);
    assert_eq!(deserialize_transaction(&unsigned).unwrap(), tx);

    let mut signed = tx.clone();
    *signed.signature_mut() = vec![0xaa; 65];
    let bytes = serialize_transaction_signed(&signed);
    assert_eq!(deserialize_transaction_signed(&bytes).unwrap(), signed);
}

#[test]
fn call_roundtrip() {
    let tx = Transaction::call("alice", "bob", vec![1, 2, 3, 4], 7, 100_000, 5, CHAIN_ID_REGTEST);
    let unsigned = serialize_transaction(&tx);
    assert_eq!(deserialize_transaction(&unsigned).unwrap(), tx);

    let mut signed = tx.clone();
    *signed.signature_mut() = vec![0xbb; 65];
    let bytes = serialize_transaction_signed(&signed);
    assert_eq!(deserialize_transaction_signed(&bytes).unwrap(), signed);
}

#[test]
fn variants_encode_differently() {
    let transfer = serialize_transaction(&Transaction::transfer("a", "b", 1, 1, 1));
    let create = serialize_transaction(&Transaction::create("a", vec![1], vec![], 0, 1, 1));
    let call = serialize_transaction(&Transaction::call("a", "b", vec![1], 0, 1, 1, 1));
    assert_ne!(transfer, create);
    assert_ne!(transfer, call);
    assert_ne!(create, call);
}

// ---------------------------------------------------------------------------
// Signature verification per variant
// ---------------------------------------------------------------------------

#[test]
fn verify_transfer_signed_ok() {
    let (addr, sk) = keypair();
    let mut tx = Transaction::transfer(&addr, "bob", 100, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    verify_transaction(&tx).expect("valid signed transfer must verify");
}

#[test]
fn verify_create_signed_ok() {
    let (addr, sk) = keypair();
    let mut tx = Transaction::create(&addr, vec![0x00, 0x61, 0x73, 0x6d], vec![], 0, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    verify_transaction(&tx).expect("valid signed create must verify");
}

#[test]
fn verify_call_signed_ok() {
    let (addr, sk) = keypair();
    let mut tx = Transaction::call(&addr, "contract1", vec![1, 2, 3], 10, 50_000, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    verify_transaction(&tx).expect("valid signed call must verify");
}

#[test]
fn verify_rejects_tampered_payload() {
    let (addr, sk) = keypair();
    let mut tx = Transaction::transfer(&addr, "bob", 100, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    // Payload is part of the signed bytes: any mutation invalidates the sig.
    if let Transaction::Transfer(t) = &mut tx {
        t.amount = 101;
    }
    assert!(matches!(
        verify_transaction(&tx),
        Err(CoreError::InvalidSignature)
    ));
}

#[test]
fn verify_rejects_tampered_call_data() {
    let (addr, sk) = keypair();
    let mut tx = Transaction::call(&addr, "contract1", vec![1, 2, 3], 10, 50_000, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    if let Transaction::Call(t) = &mut tx {
        t.data = vec![9, 9, 9];
    }
    assert!(matches!(
        verify_transaction(&tx),
        Err(CoreError::InvalidSignature)
    ));
}

#[test]
fn verify_rejects_tampered_init_code() {
    let (addr, sk) = keypair();
    let mut tx = Transaction::create(&addr, vec![1, 2, 3], vec![], 0, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    if let Transaction::Create(t) = &mut tx {
        t.init_code = vec![4, 5, 6];
    }
    assert!(matches!(
        verify_transaction(&tx),
        Err(CoreError::InvalidSignature)
    ));
}

#[test]
fn coinbase_bypasses_signature() {
    let tx = Transaction::coinbase("coinbase", "miner", 50, CHAIN_ID_REGTEST);
    verify_transaction(&tx).expect("coinbase must skip signature verification");
}

// ---------------------------------------------------------------------------
// Shape validation
// ---------------------------------------------------------------------------

#[test]
fn create_with_empty_init_code_rejected() {
    let (addr, sk) = keypair();
    let mut tx = Transaction::create(&addr, Vec::new(), vec![1], 0, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    assert!(matches!(
        verify_transaction(&tx),
        Err(CoreError::InvalidContractDeploy { .. })
    ));
}

#[test]
fn call_with_zero_gas_limit_rejected() {
    let (addr, sk) = keypair();
    let mut tx = Transaction::call(&addr, "contract1", vec![], 0, 0, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    assert!(matches!(
        verify_transaction(&tx),
        Err(CoreError::InvalidGasLimit { .. })
    ));
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

#[test]
fn accessors_dispatch_per_variant() {
    let transfer = Transaction::transfer("a", "b", 11, 1, 1);
    assert_eq!(transfer.sender(), "a");
    assert_eq!(transfer.receiver(), Some("b"));
    assert_eq!(transfer.value(), 11);
    assert_eq!(transfer.nonce(), 1);
    assert_eq!(transfer.chain_id(), 1);
    assert_eq!(transfer.fee(), 0);
    assert!(!transfer.is_coinbase());

    let create = Transaction::create("a", vec![1], vec![], 22, 2, 1);
    assert_eq!(create.sender(), "a");
    assert_eq!(create.receiver(), None);
    assert_eq!(create.value(), 22);
    assert_eq!(create.nonce(), 2);
    assert!(!create.is_coinbase());

    let call = Transaction::call("a", "c", vec![1], 33, 1000, 3, 1);
    assert_eq!(call.sender(), "a");
    assert_eq!(call.receiver(), Some("c"));
    assert_eq!(call.value(), 33);
    assert_eq!(call.nonce(), 3);

    let coinbase = Transaction::coinbase("coinbase", "miner", 50, 1);
    assert!(coinbase.is_coinbase());
    assert_eq!(coinbase.value(), 50);
}

// ---------------------------------------------------------------------------
// Serde compatibility
// ---------------------------------------------------------------------------

#[test]
fn serde_legacy_flat_json_deserializes_as_transfer() {
    let legacy = r#"{"sender":"alice","receiver":"bob","amount":100,"nonce":1,"chain_id":3,"signature":[1,2],"is_coinbase":false}"#;
    let tx: Transaction = serde_json::from_str(legacy).expect("legacy JSON must parse");
    match &tx {
        Transaction::Transfer(t) => {
            assert_eq!(t.from, "alice");
            assert_eq!(t.to, "bob");
            assert_eq!(t.amount, 100);
            assert_eq!(t.fee, 0, "legacy blobs without fee default to 0");
            assert_eq!(t.nonce, 1);
            assert_eq!(t.chain_id, 3);
            assert_eq!(t.signature, vec![1, 2]);
            assert!(!t.is_coinbase);
        }
        other => panic!("expected Transfer, got {other:?}"),
    }
}

#[test]
fn serde_tagged_create_and_call_roundtrip() {
    let create = Transaction::create("alice", vec![1, 2, 3], vec![4], 50, 1, 3);
    let json = serde_json::to_string(&create).unwrap();
    assert!(json.contains("\"type\":\"create\""), "{json}");
    let back: Transaction = serde_json::from_str(&json).unwrap();
    assert_eq!(back, create);

    let call = Transaction::call("alice", "bob", vec![9], 7, 100_000, 1, 3);
    let json = serde_json::to_string(&call).unwrap();
    assert!(json.contains("\"type\":\"call\""), "{json}");
    let back: Transaction = serde_json::from_str(&json).unwrap();
    assert_eq!(back, call);
}

#[test]
fn serde_transfer_serializes_with_legacy_field_names() {
    let tx = Transaction::transfer("alice", "bob", 100, 1, 3);
    let json = serde_json::to_string(&tx).unwrap();
    assert!(json.contains("\"sender\":\"alice\""), "{json}");
    assert!(json.contains("\"receiver\":\"bob\""), "{json}");
    assert!(!json.contains("\"type\""), "transfer must stay untagged: {json}");
}

// ---------------------------------------------------------------------------
// apply_block gate (execution deferred to S1.5-P15)
// ---------------------------------------------------------------------------

fn regtest_block(index: u64, transactions: Vec<Transaction>) -> Block {
    let txs = if index > 0 {
        let mut v = vec![Transaction::coinbase("coinbase", "miner", 0, CHAIN_ID_REGTEST)];
        v.extend(transactions);
        v
    } else {
        transactions
    };
    Block {
        index,
        timestamp: 1000 + index * 600,
        transactions: txs,
        previous_hash: "prev".to_string(),
        hash: format!("block_{index}"),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

#[test]
fn apply_block_rejects_create_and_call() {
    let mut state = State::new();
    state.set_balance("alice", 10_000);

    let create_block = regtest_block(
        1,
        vec![Transaction::create("alice", vec![1, 2, 3], vec![], 100, 1, CHAIN_ID_REGTEST)],
    );
    let err = apply_block(&state, &create_block, CHAIN_ID_REGTEST).unwrap_err();
    assert!(matches!(err, CoreError::UnsupportedTxType { .. }), "{err:?}");

    let call_block = regtest_block(
        1,
        vec![Transaction::call("alice", "contract1", vec![], 10, 100_000, 1, CHAIN_ID_REGTEST)],
    );
    let err = apply_block(&state, &call_block, CHAIN_ID_REGTEST).unwrap_err();
    assert!(matches!(err, CoreError::UnsupportedTxType { .. }), "{err:?}");
}

#[test]
fn transfer_apply_unapply_roundtrip_still_works() {
    let state = State::new();
    let genesis = regtest_block(0, vec![Transaction::coinbase("genesis", "alice", 1000, CHAIN_ID_REGTEST)]);
    let after_genesis = apply_block(&state, &genesis, CHAIN_ID_REGTEST).unwrap();

    let block = regtest_block(
        1,
        vec![Transaction::transfer("alice", "bob", 300, 1, CHAIN_ID_REGTEST)],
    );
    let after = apply_block(&after_genesis, &block, CHAIN_ID_REGTEST).unwrap();
    assert_eq!(after.get_balance("alice"), 700);
    assert_eq!(after.get_balance("bob"), 300);

    let restored = unapply_block(&after, &block).unwrap();
    assert_eq!(restored, after_genesis);
}

// ---------------------------------------------------------------------------
// Struct-shape sanity (variant structs exist with expected fields)
// ---------------------------------------------------------------------------

#[test]
fn variant_structs_are_constructible() {
    let t = TransferTx {
        from: "a".into(),
        to: "b".into(),
        amount: 1,
        fee: 2,
        nonce: 3,
        chain_id: 4,
        signature: vec![],
        is_coinbase: false,
    };
    assert_eq!(t.fee, 2);

    let c = CreateTx {
        from: "a".into(),
        init_code: vec![1],
        init_args: vec![2],
        endowment: 3,
        fee: 4,
        nonce: 5,
        chain_id: 6,
        signature: vec![],
    };
    assert_eq!(c.endowment, 3);

    let k = CallTx {
        from: "a".into(),
        to: "b".into(),
        data: vec![1],
        value: 2,
        gas_limit: 3,
        fee: 4,
        nonce: 5,
        chain_id: 6,
        signature: vec![],
    };
    assert_eq!(k.gas_limit, 3);
}
