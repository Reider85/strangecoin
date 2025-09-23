use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey, SIGNATURE_LENGTH};
use pbkdf2::{
    password_hash::{
        rand_core::RngCore, PasswordHash, PasswordHasher, SaltString,
    },
    Pbkdf2,
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use uuid::Uuid;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

#[derive(Serialize, Deserialize)]
pub struct Keystore {
    pub public_key: String,
    pub encrypted_private_key: String,
    pub salt: String,
    pub nonce: String,
    pub version: String,
}

pub struct Wallet {
    pub public_key: VerifyingKey,
    pub private_key: Option<SigningKey>,
}

impl Wallet {
    pub fn new(password: &str, config_path: &Path) -> Result<Self, String> {
        let start_time = std::time::SystemTime::now();
        // Generate Ed25519 key pair
        let mut csprng = OsRng;
        let signing_key: SigningKey = SigningKey::generate(&mut csprng);
        let public_key = signing_key.verifying_key();
        let private_key_bytes = signing_key.to_bytes();

        // Generate salt for PBKDF2
        let salt = SaltString::generate(&mut csprng);

        // Derive key from password using PBKDF2
        let password_hash = Pbkdf2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| format!("Password hashing error: {}", e))?;
        let derived_key = password_hash.hash.ok_or("Failed to obtain derived key")?;
        let derived_key_bytes = derived_key.as_bytes();

        // Encrypt private key using AES-256-GCM
        let cipher = Aes256Gcm::new_from_slice(derived_key_bytes)
            .map_err(|e| format!("AES-256-GCM cipher creation error: {}", e))?;
        let mut nonce_bytes = [0u8; 12];
        csprng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, private_key_bytes.as_ref())
            .map_err(|e| format!("Private key encryption error: {}", e))?;

        // Encode data in Base64 for storage
        let public_key_base64 = BASE64.encode(public_key.as_bytes());
        let encrypted_private_key_base64 = BASE64.encode(&ciphertext);
        let nonce_base64 = BASE64.encode(nonce.as_slice());

        // Sanitize Base64 string for filename (replace invalid characters)
        let sanitized_public_key = public_key_base64
            .replace("/", "_")
            .replace("+", "_")
            .replace("=", "_");

        // Create Keystore structure
        let keystore = Keystore {
            public_key: public_key_base64.clone(),
            encrypted_private_key: encrypted_private_key_base64,
            salt: salt.to_string(),
            nonce: nonce_base64,
            version: "1.0".to_string(),
        };

        // Save Keystore to file with sanitized filename
        let keystore_path = config_path
            .parent()
            .ok_or("Failed to determine configuration directory")?
            .join(format!("wallet_{}.json", sanitized_public_key));
        let keystore_json = serde_json::to_string_pretty(&keystore)
            .map_err(|e| format!("Keystore serialization error: {}", e))?;
        fs::write(&keystore_path, keystore_json)
            .map_err(|e| format!("Error writing Keystore to file {}: {}", keystore_path.display(), e))?;

        // Update config.json with the new public key
        let config_content = fs::read_to_string(config_path)
            .unwrap_or(r#"{"wallet": {"name": "", "password": "", "port": 8081, "ip": "127.0.0.1"}}"#.to_string());
        let mut config: super::Config = serde_json::from_str(&config_content)
            .map_err(|e| format!("Error parsing config.json: {}", e))?;
        config.wallet.name = public_key_base64.clone();
        let config_json = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Error serializing config.json: {}", e))?;
        fs::write(config_path, config_json)
            .map_err(|e| format!("Error writing config.json: {}", e))?;

        let duration = std::time::SystemTime::now()
            .duration_since(start_time)
            .map_err(|e| format!("Time calculation error: {}", e))?
            .as_secs_f64();
        println!("Wallet created in {} seconds, public key: {}", duration, public_key_base64);

        Ok(Wallet {
            public_key,
            private_key: Some(signing_key),
        })
    }

    pub fn load(password: &str, config_path: &Path) -> Result<Self, String> {
        let start_time = std::time::SystemTime::now();
        // Read config.json to get public key
        let config_content = fs::read_to_string(config_path)
            .map_err(|e| format!("Error reading config.json: {}", e))?;
        let config: super::Config = serde_json::from_str(&config_content)
            .map_err(|e| format!("Error parsing config.json: {}", e))?;
        let public_key_base64 = config.wallet.name;

        // Sanitize Base64 string for filename (replace invalid characters)
        let sanitized_public_key = public_key_base64
            .replace("/", "_")
            .replace("+", "_")
            .replace("=", "_");

        // Read Keystore
        let keystore_path = config_path
            .parent()
            .ok_or("Failed to determine configuration directory")?
            .join(format!("wallet_{}.json", sanitized_public_key));
        let keystore_content = fs::read_to_string(&keystore_path)
            .map_err(|e| format!("Error reading Keystore {}: {}", keystore_path.display(), e))?;
        let keystore: Keystore = serde_json::from_str(&keystore_content)
            .map_err(|e| format!("Error parsing Keystore: {}", e))?;

        // Decode data from Base64
        let public_key_bytes = BASE64.decode(&keystore.public_key)
            .map_err(|e| format!("Error decoding public key: {}", e))?;
        let encrypted_private_key = BASE64.decode(&keystore.encrypted_private_key)
            .map_err(|e| format!("Error decoding encrypted private key: {}", e))?;
        let nonce_bytes = BASE64.decode(&keystore.nonce)
            .map_err(|e| format!("Error decoding nonce: {}", e))?;
        let salt = SaltString::new(&keystore.salt)
            .map_err(|e| format!("Error parsing salt: {}", e))?;

        // Restore public key
        let public_key_array: [u8; 32] = public_key_bytes
            .try_into()
            .map_err(|_| "Public key has invalid length (expected 32 bytes)")?;
        let public_key = VerifyingKey::from_bytes(&public_key_array)
            .map_err(|e| format!("Error restoring public key: {}", e))?;

        // Derive key from password
        let password_hash = Pbkdf2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| format!("Error hashing password: {}", e))?;
        let derived_key = password_hash.hash.ok_or("Failed to obtain derived key")?;
        let derived_key_bytes = derived_key.as_bytes();
        // Decrypt private key
        let cipher = Aes256Gcm::new_from_slice(derived_key_bytes)
            .map_err(|e| format!("Error creating AES-256-GCM cipher: {}", e))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let private_key_bytes = cipher
            .decrypt(nonce, encrypted_private_key.as_ref())
            .map_err(|e| format!("Error decrypting private key: {}", e))?;

        // Convert Vec<u8> to [u8; 32]
        let private_key_array: [u8; 32] = private_key_bytes
            .try_into()
            .map_err(|_| "Private key has invalid length (expected 32 bytes)")?;
        let signing_key = SigningKey::from_bytes(&private_key_array);

        let duration = std::time::SystemTime::now()
            .duration_since(start_time)
            .map_err(|e| format!("Time calculation error: {}", e))?
            .as_secs_f64();
        println!("Wallet loaded in {} seconds, public key: {}", duration, public_key_base64);

        Ok(Wallet {
            public_key,
            private_key: Some(signing_key),
        })
    }

    pub fn sign_transaction(&self, transaction: &super::Transaction) -> Result<String, String> {
        let signing_key = self.private_key.as_ref().ok_or("Private key is missing")?;
        let message = format!(
            "{}{}{}{}",
            transaction.id,
            transaction.sender,
            transaction.receiver,
            transaction.amount
        );
        let signature = signing_key
            .sign(message.as_bytes())
            .to_bytes();
        Ok(BASE64.encode(signature))
    }
}