//! BUG-S1-009 — Wallet lifecycle round-trip (P04 КГ).
//!
//! The full keystore cycle must hold end-to-end: `Wallet::new` persists an
//! encrypted keystore, `Wallet::load` restores the same keypair under the
//! correct password, and signatures from the restored wallet verify against
//! its public key. Negative paths must fail closed: wrong password, corrupted
//! keystore JSON, tampered ciphertext (AES-GCM auth), and a foreign public key
//! must never verify the signature.

mod common;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use common::*;
use strangecoin::error::StrangecoinError;
use strangecoin::wallet::Wallet;

const PASSWORD: &str = "s3cure-roundtrip-password";

fn keystore_of(dir: &TestDir) -> std::path::PathBuf {
    let mut keystores = Wallet::list_keystores(dir.path()).expect("list_keystores");
    assert_eq!(keystores.len(), 1, "exactly one keystore must exist");
    keystores.pop().unwrap()
}

#[test]
fn wallet_new_save_load_sign_verify_roundtrip() {
    let dir = TestDir::new("wallet_roundtrip");

    let wallet = Wallet::new(PASSWORD, dir.path()).expect("Wallet::new must succeed");
    let keystore_path = keystore_of(&dir);
    assert!(
        keystore_path.exists(),
        "Wallet::new must persist the keystore file"
    );

    let loaded = Wallet::load(PASSWORD, &keystore_path).expect("Wallet::load must succeed");
    assert_eq!(
        loaded.public_key, wallet.public_key,
        "restored public key must match the created one"
    );

    let digest = *blake3::hash(b"BUG-S1-009 wallet round-trip").as_bytes();
    let sig = wallet.sign(&digest).expect("sign must succeed");
    let compact: [u8; 64] = sig[..64].try_into().expect("compact signature");
    assert!(
        Wallet::verify(&compact, &digest, &loaded.public_key),
        "signature from the created wallet must verify with the loaded public key"
    );
}

#[test]
fn wallet_load_rejects_wrong_password() {
    let dir = TestDir::new("wallet_wrong_password");
    Wallet::new(PASSWORD, dir.path()).expect("Wallet::new must succeed");
    let keystore_path = keystore_of(&dir);

    let err = Wallet::load("definitely-not-the-password", &keystore_path)
        .map(drop)
        .expect_err("wrong password must be rejected");
    assert!(
        matches!(err, StrangecoinError::ConfigError(_)),
        "wrong password must surface as ConfigError, got: {err:?}"
    );
}

#[test]
fn wallet_load_rejects_corrupted_keystore() {
    let dir = TestDir::new("wallet_corrupted");
    Wallet::new(PASSWORD, dir.path()).expect("Wallet::new must succeed");
    let keystore_path = keystore_of(&dir);

    std::fs::write(&keystore_path, b"not-a-json-keystore{{{").expect("overwrite keystore");
    let err = Wallet::load(PASSWORD, &keystore_path)
        .map(drop)
        .expect_err("corrupted keystore JSON must be rejected");
    assert!(
        matches!(err, StrangecoinError::ConfigError(_)),
        "corrupted keystore must surface as ConfigError, got: {err:?}"
    );
}

#[test]
fn wallet_load_rejects_tampered_ciphertext() {
    let dir = TestDir::new("wallet_tampered_ct");
    Wallet::new(PASSWORD, dir.path()).expect("Wallet::new must succeed");
    let keystore_path = keystore_of(&dir);

    // Flip one ciphertext byte via decode/re-encode: base64 stays valid, so
    // load reaches the AES-GCM decrypt and fails the auth-tag check. The
    // nonce and salt are left intact (`Nonce::from_slice` panics on a wrong
    // length, and the salt is not secret material worth tampering here).
    let content = std::fs::read_to_string(&keystore_path).expect("read keystore");
    let mut json: serde_json::Value =
        serde_json::from_str(&content).expect("keystore must be valid JSON");
    let ct_b64 = json["encrypted_private_key"]
        .as_str()
        .expect("encrypted_private_key field")
        .to_string();
    let mut ciphertext = BASE64.decode(&ct_b64).expect("ciphertext base64");
    assert!(!ciphertext.is_empty(), "ciphertext must not be empty");
    ciphertext[0] ^= 0xFF;
    json["encrypted_private_key"] = serde_json::json!(BASE64.encode(&ciphertext));
    std::fs::write(
        &keystore_path,
        serde_json::to_vec(&json).expect("serialize"),
    )
    .expect("write tampered keystore");

    let err = Wallet::load(PASSWORD, &keystore_path)
        .map(drop)
        .expect_err("tampered ciphertext must be rejected by AES-GCM auth");
    assert!(
        matches!(err, StrangecoinError::ConfigError(_)),
        "tampered ciphertext must surface as ConfigError, got: {err:?}"
    );
}

#[test]
fn wallet_sign_verify_with_wrong_pubkey_rejected() {
    let dir = TestDir::new("wallet_wrong_pubkey");
    let wallet = Wallet::new(PASSWORD, dir.path()).expect("Wallet::new must succeed");

    let digest = *blake3::hash(b"BUG-S1-009 foreign pubkey").as_bytes();
    let sig = wallet.sign(&digest).expect("sign must succeed");
    let compact: [u8; 64] = sig[..64].try_into().expect("compact signature");

    assert!(
        Wallet::verify(&compact, &digest, &wallet.public_key),
        "sanity: own public key must verify"
    );

    let (_, other_key) = generate_keypair();
    let other_pk = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), &other_key);
    assert!(
        !Wallet::verify(&compact, &digest, &other_pk),
        "a foreign public key must not verify the signature"
    );
}
