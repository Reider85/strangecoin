//! # Lock Ordering
//!
//! To prevent deadlocks, locks must always be acquired in this order:
//!
//! 1. **blockchain** (via `BlockchainFacade`, internally `RwLock<Blockchain>`) — outer, first.
//!    The blockchain is the top-level container owning chain, balances, mempool.
//!    See `ARCHITECT3.md §3.4`: state_cache is the sole read path for balances,
//!    facade is the sole entry point.
//!
//! 2. **wallet** (file-based keystore lock, future in-memory lock) — inner, second.
//!    Wallet is a peripheral entity; access occurs in context of a known account.
//!
//! Never acquire wallet lock while holding blockchain write lock from a different call site.
//! Read locks on blockchain may be held while acquiring wallet lock.
//! This ordering is enforced by the deadlock test in `tests/concurrency.rs`.

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
    #[error("peer banned: rate limit exceeded")]
    PeerBanned,
    #[error("hex decode error: {0}")]
    HexError(#[from] hex::FromHexError),
    #[error("secp256k1 error: {0}")]
    Secp256k1Error(#[from] secp256k1::Error),
    #[error("invalid coinbase amount: expected {expected}, got {got}")]
    InvalidCoinbaseAmount { expected: u64, got: u64 },
    #[error("mempool full: max {0} transactions")]
    MempoolFull(usize),
    #[error("duplicate transaction")]
    DuplicateTx,
    #[error("insufficient balance: have {available}, need {required}")]
    InsufficientBalance { available: u64, required: u64 },
    #[error("config error: {0}")]
    ConfigError(String),
    #[error("grant blocks are disabled (allow_grant_blocks = false)")]
    GrantBlocksDisabled,
    #[error("invalid block: {0}")]
    InvalidBlock(String),
}

impl From<strangecoin_core::CoreError> for StrangecoinError {
    fn from(e: strangecoin_core::CoreError) -> Self {
        match e {
            strangecoin_core::CoreError::InvalidSignature => StrangecoinError::InvalidSignature,
            strangecoin_core::CoreError::InvalidNonce { expected, got } => {
                StrangecoinError::InvalidNonce { expected, got }
            }
            strangecoin_core::CoreError::InvalidChainId { expected, got } => {
                StrangecoinError::InvalidChainId { expected, got }
            }
            strangecoin_core::CoreError::InvalidDifficulty => StrangecoinError::InvalidDifficulty,
            strangecoin_core::CoreError::TimestampTooOld => StrangecoinError::TimestampTooOld,
            strangecoin_core::CoreError::TimestampInFuture => StrangecoinError::TimestampInFuture,
            strangecoin_core::CoreError::GenesisMismatch { expected, got } => {
                StrangecoinError::GenesisMismatch { expected, got }
            }
            strangecoin_core::CoreError::HexError(e) => StrangecoinError::HexError(e),
            strangecoin_core::CoreError::Secp256k1Error(e) => StrangecoinError::Secp256k1Error(e),
            strangecoin_core::CoreError::InvalidCoinbaseAmount { expected, got } => {
                StrangecoinError::InvalidCoinbaseAmount { expected, got }
            }
            strangecoin_core::CoreError::InsufficientBalance {
                sender: _,
                available,
                required,
            } => StrangecoinError::InsufficientBalance {
                available,
                required,
            },
            strangecoin_core::CoreError::StateOverflow => {
                StrangecoinError::ConfigError("arithmetic overflow in state transition".to_string())
            }
            strangecoin_core::CoreError::StateRootMismatch { expected, got } => {
                StrangecoinError::ConfigError(format!(
                    "state root mismatch: expected {:?}, got {:?}",
                    expected, got
                ))
            }
            strangecoin_core::CoreError::TxRootMismatch { expected, got } => {
                StrangecoinError::ConfigError(format!(
                    "tx root mismatch: expected {:?}, got {:?}",
                    expected, got
                ))
            }
            strangecoin_core::CoreError::WitnessVerificationFailed => {
                StrangecoinError::ConfigError("witness verification failed".to_string())
            }
        }
    }
}
