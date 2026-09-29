use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },
    #[error("invalid chain_id: expected {expected}, got {got}")]
    InvalidChainId { expected: u32, got: u32 },
    #[error("block difficulty mismatch")]
    InvalidDifficulty,
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
    #[error("insufficient balance for {sender}: have {available}, need {required}")]
    InsufficientBalance {
        sender: String,
        available: u64,
        required: u64,
    },
    #[error("arithmetic overflow in state transition")]
    StateOverflow,
    #[error("state root mismatch: expected {expected:?}, got {got:?}")]
    StateRootMismatch { expected: [u8; 32], got: [u8; 32] },
    #[error("tx root mismatch: expected {expected:?}, got {got:?}")]
    TxRootMismatch { expected: [u8; 32], got: [u8; 32] },
    #[error("witness verification failed")]
    WitnessVerificationFailed,
}
