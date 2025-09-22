use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey, SIGNATURE_LENGTH};  // SigningKey вместо Keypair, VerifyingKey вместо PublicKey
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
    pub public_key: VerifyingKey,  // VerifyingKey вместо PublicKey
    pub private_key: Option<SigningKey>,  // SigningKey вместо SecretKey для приватной части
}

impl Wallet {
    pub fn new(password: &str, config_path: &Path) -> Result<Self, String> {
        let start_time = std::time::SystemTime::now();
        // Генерация пары ключей Ed25519
        let mut csprng = OsRng;
        let signing_key: SigningKey = SigningKey::generate(&mut csprng);  // SigningKey::generate вместо Keypair::generate
        let public_key = signing_key.verifying_key();  // Извлекаем VerifyingKey из SigningKey
        let private_key_bytes = signing_key.to_bytes();  // to_bytes() для всего SigningKey

        // Генерация соли для PBKDF2
        let salt = SaltString::generate(&mut csprng);

        // Производный ключ от пароля с использованием PBKDF2
        let password_hash = Pbkdf2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| format!("Ошибка хеширования пароля: {}", e))?;
        let derived_key = password_hash.hash.ok_or("Не удалось получить производный ключ")?;
        let derived_key_bytes = derived_key.as_bytes();

        // Шифрование приватного ключа с использованием AES-256-GCM
        let cipher = Aes256Gcm::new_from_slice(derived_key_bytes)
            .map_err(|e| format!("Ошибка создания шифра AES-256-GCM: {}", e))?;
        // Создание nonce
        let mut nonce_bytes = [0u8; 12];
        csprng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, private_key_bytes.as_ref())
            .map_err(|e| format!("Ошибка шифрования приватного ключа: {}", e))?;

        // Кодирование данных в Base64 для хранения
        let public_key_base64 = BASE64.encode(public_key.as_bytes());  // as_bytes() вместо to_bytes()
        let encrypted_private_key_base64 = BASE64.encode(&ciphertext);
        let nonce_base64 = BASE64.encode(nonce.as_slice());

        // Создание структуры Keystore
        let keystore = Keystore {
            public_key: public_key_base64.clone(),
            encrypted_private_key: encrypted_private_key_base64,
            salt: salt.to_string(),
            nonce: nonce_base64,
            version: "1.0".to_string(),
        };

        // Сохранение Keystore в файл
        let keystore_path = config_path
            .parent()
            .ok_or("Не удалось определить директорию конфигурации")?
            .join(format!("wallet_{}.json", public_key_base64));
        let keystore_json = serde_json::to_string_pretty(&keystore)
            .map_err(|e| format!("Ошибка сериализации Keystore: {}", e))?;
        fs::write(&keystore_path, keystore_json)
            .map_err(|e| format!("Ошибка записи Keystore в файл {}: {}", keystore_path.display(), e))?;

        // Обновление config.json с новым публичным ключом
        let config_content = fs::read_to_string(config_path)
            .unwrap_or(r#"{"wallet": {"name": "", "password": "", "port": 8081, "ip": "127.0.0.1"}}"#.to_string());
        let mut config: super::Config = serde_json::from_str(&config_content)
            .map_err(|e| format!("Ошибка парсинга config.json: {}", e))?;
        config.wallet.name = public_key_base64.clone();
        let config_json = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Ошибка сериализации config.json: {}", e))?;
        fs::write(config_path, config_json)
            .map_err(|e| format!("Ошибка записи config.json: {}", e))?;

        let duration = std::time::SystemTime::now()
            .duration_since(start_time)
            .map_err(|e| format!("Ошибка вычисления времени: {}", e))?
            .as_secs_f64();
        println!("Кошелёк создан за {} секунд, публичный ключ: {}", duration, public_key_base64);

        Ok(Wallet {
            public_key,
            private_key: Some(signing_key),  // SigningKey вместо SecretKey
        })
    }

    pub fn load(password: &str, config_path: &Path) -> Result<Self, String> {
        let start_time = std::time::SystemTime::now();
        // Чтение config.json для получения публичного ключа
        let config_content = fs::read_to_string(config_path)
            .map_err(|e| format!("Ошибка чтения config.json: {}", e))?;
        let config: super::Config = serde_json::from_str(&config_content)
            .map_err(|e| format!("Ошибка парсинга config.json: {}", e))?;
        let public_key_base64 = config.wallet.name;

        // Чтение Keystore
        let keystore_path = config_path
            .parent()
            .ok_or("Не удалось определить директорию конфигурации")?
            .join(format!("wallet_{}.json", public_key_base64));
        let keystore_content = fs::read_to_string(&keystore_path)
            .map_err(|e| format!("Ошибка чтения Keystore {}: {}", keystore_path.display(), e))?;
        let keystore: Keystore = serde_json::from_str(&keystore_content)
            .map_err(|e| format!("Ошибка парсинга Keystore: {}", e))?;

        // Декодирование данных из Base64
        let public_key_bytes = BASE64.decode(&keystore.public_key)
            .map_err(|e| format!("Ошибка декодирования публичного ключа: {}", e))?;
        let encrypted_private_key = BASE64.decode(&keystore.encrypted_private_key)
            .map_err(|e| format!("Ошибка декодирования зашифрованного приватного ключа: {}", e))?;
        let nonce_bytes = BASE64.decode(&keystore.nonce)
            .map_err(|e| format!("Ошибка декодирования nonce: {}", e))?;
        let salt = SaltString::new(&keystore.salt)
            .map_err(|e| format!("Ошибка парсинга соли: {}", e))?;

        // Восстановление публичного ключа
        let public_key_array: [u8; 32] = public_key_bytes
            .try_into()
            .map_err(|_| "Публичный ключ имеет неверную длину (ожидалось 32 байта)")?;
        let public_key = VerifyingKey::from_bytes(&public_key_array)
            .map_err(|e| format!("Ошибка восстановления публичного ключа: {}", e))?;

        // Производный ключ от пароля
        let password_hash = Pbkdf2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| format!("Ошибка хеширования пароля: {}", e))?;
        let derived_key = password_hash.hash.ok_or("Не удалось получить производный ключ")?;
        let derived_key_bytes = derived_key.as_bytes();
        // Расшифровка приватного ключа
        let cipher = Aes256Gcm::new_from_slice(derived_key_bytes)
            .map_err(|e| format!("Ошибка создания шифра AES-256-GCM: {}", e))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let private_key_bytes = cipher
            .decrypt(nonce, encrypted_private_key.as_ref())
            .map_err(|e| format!("Ошибка расшифровки приватного ключа: {}", e))?;

        // Конвертация Vec<u8> в [u8; 32]
        let private_key_array: [u8; 32] = private_key_bytes
            .try_into()
            .map_err(|_| "Приватный ключ имеет неверную длину (ожидалось 32 байта)")?;
        let signing_key = SigningKey::from_bytes(&private_key_array); // from_bytes не возвращает Result

        let duration = std::time::SystemTime::now()
            .duration_since(start_time)
            .map_err(|e| format!("Ошибка вычисления времени: {}", e))?
            .as_secs_f64();
        println!("Кошелёк загружен за {} секунд, публичный ключ: {}", duration, public_key_base64);

        Ok(Wallet {
            public_key,
            private_key: Some(signing_key),
        })
    }

    pub fn sign_transaction(&self, transaction: &super::Transaction) -> Result<String, String> {
        let signing_key = self.private_key.as_ref().ok_or("Приватный ключ отсутствует")?;
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