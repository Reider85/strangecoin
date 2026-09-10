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
}