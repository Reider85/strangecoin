use thiserror::Error;

#[derive(Debug, Error)]
pub enum StrangecoinError {
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },
    #[error("invalid chain_id: expected {expected}, got {got}")]
    InvalidChainId { expected: u32, got: u32 },
    #[error("block difficulty mismatch")]
    InvalidDifficulty,
    #[error("size limit exceeded: {0}")]
    SizeLimitExceeded(&'static str),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("leveldb error: {0}")]
    LeveldbError(#[from] rusty_leveldb::Status),
    #[error("block timestamp too old: must be > median time past of last 11 blocks")]
    TimestampTooOld,
    #[error("block timestamp in future: must be <= now + 2 hours")]
    TimestampInFuture,
    #[error("genesis mismatch: expected {expected:?}, got {got:?}")]
    GenesisMismatch { expected: [u8; 32], got: [u8; 32] },
    #[error("hex decode error: {0}")]
    HexError(#[from] hex::FromHexError),
    #[error("secp256k1 error: {0}")]
    Secp256k1Error(#[from] secp256k1::Error),
    #[error("invalid coinbase amount: expected {expected}, got {got}")]
    InvalidCoinbaseAmount { expected: u64, got: u64 },
}