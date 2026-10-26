use crate::error::StrangecoinError;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use blake3;
use pbkdf2::{
    password_hash::{rand_core::RngCore, PasswordHasher, SaltString},
    Pbkdf2,
};
use rand::rngs::OsRng;
use secp256k1::{
    ecdsa::{RecoverableSignature, Signature},
    Message, PublicKey, Secp256k1, SecretKey,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Serialize, Deserialize)]
pub struct Keystore {
    pub public_key: String,
    pub encrypted_private_key: String,
    pub salt: String,
    pub nonce: String,
    pub version: String,
}

pub struct Wallet {
    pub public_key: PublicKey,
    pub private_key: Option<SecretKey>,
}

impl Wallet {
    pub fn new(password: &str, data_dir: &Path) -> Result<Self, StrangecoinError> {
        let start_time = std::time::SystemTime::now();
        let secp = Secp256k1::new();
        let mut csprng = OsRng;

        let secret_key = SecretKey::new(&mut csprng);
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);
        let private_key_bytes = secret_key.secret_bytes();

        let salt = SaltString::generate(&mut csprng);

        let password_hash = Pbkdf2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| StrangecoinError::ConfigError(format!("Password hashing error: {}", e)))?;
        let derived_key = password_hash
            .hash
            .ok_or_else(|| StrangecoinError::ConfigError("Failed to obtain derived key".into()))?;
        let derived_key_bytes = derived_key.as_bytes();

        let cipher = Aes256Gcm::new_from_slice(derived_key_bytes).map_err(|e| {
            StrangecoinError::ConfigError(format!("AES-256-GCM cipher creation error: {}", e))
        })?;
        let mut nonce_bytes = [0u8; 12];
        csprng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, private_key_bytes.as_ref())
            .map_err(|e| {
                StrangecoinError::ConfigError(format!("Private key encryption error: {}", e))
            })?;

        let public_key_base64 = BASE64.encode(public_key.serialize());
        let encrypted_private_key_base64 = BASE64.encode(&ciphertext);
        let nonce_base64 = BASE64.encode(nonce.as_slice());

        let sanitized_public_key = public_key_base64
            .replace("/", "_")
            .replace("+", "_")
            .replace("=", "_");

        let keystore = Keystore {
            public_key: public_key_base64.clone(),
            encrypted_private_key: encrypted_private_key_base64,
            salt: salt.to_string(),
            nonce: nonce_base64,
            version: "1.0".to_string(),
        };

        let keystore_dir = data_dir.join("keystore");
        fs::create_dir_all(&keystore_dir).map_err(|e| {
            StrangecoinError::ConfigError(format!("Failed to create keystore dir: {}", e))
        })?;

        let keystore_path = keystore_dir.join(format!("wallet_{}.json", sanitized_public_key));
        let keystore_json = serde_json::to_string_pretty(&keystore).map_err(|e| {
            StrangecoinError::ConfigError(format!("Keystore serialization error: {}", e))
        })?;
        fs::write(&keystore_path, keystore_json)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error writing Keystore: {}", e)))?;

        let duration = std::time::SystemTime::now()
            .duration_since(start_time)
            .map_err(|e| StrangecoinError::ConfigError(format!("Time calculation error: {}", e)))?
            .as_secs_f64();
        info!(duration_secs = duration, public_key = %public_key_base64, "Wallet created");

        Ok(Wallet {
            public_key,
            private_key: Some(secret_key),
        })
    }

    pub fn load(password: &str, keystore_path: &Path) -> Result<Self, StrangecoinError> {
        let start_time = std::time::SystemTime::now();
        let keystore_content = fs::read_to_string(keystore_path)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error reading Keystore: {}", e)))?;
        let keystore: Keystore = serde_json::from_str(&keystore_content)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error parsing Keystore: {}", e)))?;

        let public_key_bytes = BASE64.decode(&keystore.public_key).map_err(|e| {
            StrangecoinError::ConfigError(format!("Error decoding public key: {}", e))
        })?;
        let encrypted_private_key =
            BASE64
                .decode(&keystore.encrypted_private_key)
                .map_err(|e| {
                    StrangecoinError::ConfigError(format!(
                        "Error decoding encrypted private key: {}",
                        e
                    ))
                })?;
        let nonce_bytes = BASE64
            .decode(&keystore.nonce)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error decoding nonce: {}", e)))?;
        let salt = SaltString::from_b64(&keystore.salt)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error parsing salt: {}", e)))?;

        let public_key = PublicKey::from_slice(&public_key_bytes).map_err(|e| {
            StrangecoinError::ConfigError(format!("Error restoring public key: {}", e))
        })?;

        let password_hash = Pbkdf2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error hashing password: {}", e)))?;
        let derived_key = password_hash
            .hash
            .ok_or_else(|| StrangecoinError::ConfigError("Failed to obtain derived key".into()))?;
        let derived_key_bytes = derived_key.as_bytes();

        let cipher = Aes256Gcm::new_from_slice(derived_key_bytes).map_err(|e| {
            StrangecoinError::ConfigError(format!("Error creating AES-256-GCM cipher: {}", e))
        })?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let private_key_bytes = cipher
            .decrypt(nonce, encrypted_private_key.as_ref())
            .map_err(|e| {
                StrangecoinError::ConfigError(format!("Error decrypting private key: {}", e))
            })?;

        let private_key_array: [u8; 32] = private_key_bytes
            .try_into()
            .map_err(|_| StrangecoinError::ConfigError("Private key has invalid length".into()))?;
        let secret_key = SecretKey::from_slice(&private_key_array).map_err(|e| {
            StrangecoinError::ConfigError(format!("Error creating secret key: {}", e))
        })?;

        let public_key_base64 = BASE64.encode(public_key.serialize());
        let duration = std::time::SystemTime::now()
            .duration_since(start_time)
            .map_err(|e| StrangecoinError::ConfigError(format!("Time calculation error: {}", e)))?
            .as_secs_f64();
        info!(duration_secs = duration, public_key = %public_key_base64, "Wallet loaded");

        Ok(Wallet {
            public_key,
            private_key: Some(secret_key),
        })
    }

    pub fn list_keystores(data_dir: &Path) -> Result<Vec<PathBuf>, StrangecoinError> {
        let keystore_dir = data_dir.join("keystore");
        if !keystore_dir.exists() {
            return Ok(vec![]);
        }
        let mut keystores = Vec::new();
        for entry in fs::read_dir(&keystore_dir).map_err(|e| {
            StrangecoinError::ConfigError(format!("Failed to read keystore dir: {}", e))
        })? {
            let entry = entry.map_err(|e| {
                StrangecoinError::ConfigError(format!("Failed to read dir entry: {}", e))
            })?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                keystores.push(path);
            }
        }
        Ok(keystores)
    }

    pub fn get_password_from_env() -> Option<String> {
        std::env::var("STRANGECOIN_WALLET_PASSWORD").ok()
    }

    pub fn sign(&self, message: &[u8]) -> Result<[u8; 65], StrangecoinError> {
        let secret_key = self
            .private_key
            .as_ref()
            .ok_or(StrangecoinError::ConfigError(
                "Private key is missing".into(),
            ))?;
        let secp = Secp256k1::new();
        let msg = Message::from_digest_slice(message).map_err(|e| {
            StrangecoinError::ConfigError(format!("Invalid message for signing: {}", e))
        })?;
        let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, secret_key);
        let (rec_id, sig_bytes) = sig.serialize_compact();
        let mut out = [0u8; 65];
        out[..64].copy_from_slice(&sig_bytes);
        out[64] = rec_id.to_i32() as u8;
        Ok(out)
    }

    pub fn verify(sig: &[u8; 64], message: &[u8], pk: &PublicKey) -> bool {
        let secp = Secp256k1::new();
        let msg = match Message::from_digest_slice(message) {
            Ok(m) => m,
            Err(_) => return false,
        };
        let signature = match Signature::from_compact(sig) {
            Ok(s) => s,
            Err(_) => return false,
        };
        secp.verify_ecdsa(&msg, &signature, pk).is_ok()
    }

    pub fn sign_transaction(
        &self,
        transaction: &mut super::Transaction,
    ) -> Result<(), StrangecoinError> {
        let message_bytes = strangecoin_core::serialize::serialize_transaction(transaction);
        let message_hash = blake3::hash(&message_bytes);
        let signature = self.sign(message_hash.as_bytes())?;
        *transaction.signature_mut() = signature.to_vec();
        Ok(())
    }
}

impl Drop for Wallet {
    fn drop(&mut self) {
        tracing::info!("Wallet dropped, keystore locked");
    }
}

/// The node's long-term Ed25519 identity (`keystore/node/node_key.json`) —
/// S2-P09, internal node-to-node auth only (ARCHITECT3 §3.8), never an EOA
/// key: transaction signing stays secp256k1 (ADR-0001), and the two key
/// kinds live in segregated keystore directories (Stage 2 Pitfalls).
#[cfg(feature = "net")]
pub use strangecoin_net::noise::NodeKeypair;

/// Where the node identity file lives: `<data_dir>/keystore/node/node_key.json`.
#[cfg(feature = "net")]
pub fn node_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join("keystore").join("node").join("node_key.json")
}

/// Load the node's long-term Ed25519 identity, creating it on first start.
///
/// Unlike the wallet keystore this file is *not* password-gated: the node
/// must restore its P2P identity unattended at startup. It stays inside the
/// keystore tree (secrets policy — never `config.toml`) and inherits the data
/// directory's ACLs (0600 set explicitly on unix).
///
/// `OsRng` here is the one spot the Stage 2 determinism checklist (#2) allows
/// it: one-time keypair generation during node setup, offline, before any
/// network activity. Everything derived afterwards — Noise ephemeral keys in
/// particular — is deterministic from this seed (checklist #3, S2-P09).
#[cfg(feature = "net")]
pub fn load_or_create_node_keypair(data_dir: &Path) -> Result<NodeKeypair, StrangecoinError> {
    let path = node_key_path(data_dir);
    let seed: [u8; 32] = if path.exists() {
        let raw = fs::read_to_string(&path)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error reading node key: {e}")))?;
        let value: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error parsing node key: {e}")))?;
        let hex_seed = value.get("seed").and_then(|v| v.as_str()).ok_or_else(|| {
            StrangecoinError::ConfigError("node key file is missing `seed`".into())
        })?;
        let bytes = hex::decode(hex_seed)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error decoding node key: {e}")))?;
        <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| {
            StrangecoinError::ConfigError("node key seed must be exactly 32 bytes".into())
        })?
    } else {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let dir = path
            .parent()
            .expect("node key path always has a parent directory");
        fs::create_dir_all(dir).map_err(|e| {
            StrangecoinError::ConfigError(format!("Failed to create node keystore dir: {e}"))
        })?;
        let file = serde_json::json!({ "version": "1.0", "seed": hex::encode(seed) });
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| StrangecoinError::ConfigError(format!("Node key serialization: {e}")))?;
        fs::write(&path, json)
            .map_err(|e| StrangecoinError::ConfigError(format!("Error writing node key: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        info!(
            path = %path.display(),
            "Node Ed25519 identity created (keystore/node)"
        );
        seed
    };

    NodeKeypair::from_keystore(&seed)
        .map_err(|e| StrangecoinError::ConfigError(format!("Node keypair build failed: {e}")))
}

#[cfg(all(test, feature = "net"))]
mod node_identity_tests {
    use super::*;

    #[test]
    fn node_keypair_keystore_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("strangecoin-nodekey-{}", uuid::Uuid::new_v4()));

        let first = load_or_create_node_keypair(&dir).expect("first load creates the identity");
        assert!(
            node_key_path(&dir).exists(),
            "identity persisted under keystore/node/"
        );

        let second = load_or_create_node_keypair(&dir).expect("second load reads the identity");
        assert_eq!(
            first.node_id(),
            second.node_id(),
            "reload must reproduce the same node_id"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
