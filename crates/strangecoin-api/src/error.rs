//! Error types for the agent API layer

use thiserror::Error;

/// Result type for agent API operations
pub type AgentResult<T> = Result<T, AgentError>;

/// Error types for agent API operations
#[derive(Debug, Error)]
pub enum AgentError {
    /// Session not found or invalid
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// Session expired
    #[error("Session expired: {0}")]
    SessionExpired(String),

    /// Invalid session token
    #[error("Invalid session token")]
    InvalidSessionToken,

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Invalid request parameters
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    /// Transaction validation failed
    #[error("Transaction validation failed: {0}")]
    TransactionValidation(String),

    /// Batch processing failed
    #[error("Batch processing failed: {0}")]
    BatchProcessing(String),

    /// Blockchain operation failed
    #[error("Blockchain operation failed: {0}")]
    BlockchainOperation(String),

    /// Internal server error
    #[error("Internal server error: {0}")]
    InternalError(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Method not found
    #[error("Method not found: {0}")]
    MethodNotFound(String),

    /// Session limit exceeded
    #[error("Session limit exceeded")]
    SessionLimitExceeded,

    /// Sponsorship not found
    #[error("Sponsorship not found: {0}")]
    SponsorshipNotFound(String),

    /// Invalid signature
    #[error("Invalid signature")]
    InvalidSignature,

    /// Agent not found
    #[error("Agent not found: {0}")]
    AgentNotFound(String),
}

impl AgentError {
    /// Convert to JSON-RPC error code
    pub fn json_rpc_code(&self) -> i32 {
        match self {
            AgentError::SessionNotFound(_) => -32001,
            AgentError::SessionExpired(_) => -32002,
            AgentError::InvalidSessionToken => -32003,
            AgentError::AuthenticationFailed(_) => -32004,
            AgentError::PermissionDenied(_) => -32005,
            AgentError::InvalidParameters(_) => -32006,
            AgentError::TransactionValidation(_) => -32007,
            AgentError::BatchProcessing(_) => -32008,
            AgentError::BlockchainOperation(_) => -32009,
            AgentError::InternalError(_) => -32010,
            AgentError::RateLimitExceeded => -32011,
            AgentError::MethodNotFound(_) => -32012,
            AgentError::SessionLimitExceeded => -32013,
            AgentError::SponsorshipNotFound(_) => -32014,
            AgentError::InvalidSignature => -32015,
            AgentError::AgentNotFound(_) => -32016,
        }
    }

    /// Create a session not found error
    pub fn session_not_found(session_id: &str) -> Self {
        AgentError::SessionNotFound(session_id.to_string())
    }

    /// Create a permission denied error
    pub fn permission_denied(method: &str) -> Self {
        AgentError::PermissionDenied(format!("Access to '{}' denied", method))
    }

    /// Create an invalid parameters error
    pub fn invalid_parameters(detail: &str) -> Self {
        AgentError::InvalidParameters(detail.to_string())
    }

    /// Create a blockchain operation error
    pub fn blockchain_operation(detail: &str) -> Self {
        AgentError::BlockchainOperation(detail.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_codes() {
        assert_eq!(
            AgentError::session_not_found("test").json_rpc_code(),
            -32001
        );
        assert_eq!(
            AgentError::permission_denied("test").json_rpc_code(),
            -32005
        );
        assert_eq!(
            AgentError::invalid_parameters("test").json_rpc_code(),
            -32006
        );
    }
}
