//! Agent authentication and session token management

use blake3::Hasher;
use hex;
use secp256k1::Secp256k1;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{AgentError, AgentResult};
use strangecoin_core::types::AgentId;

/// Session token for agent authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    /// Session identifier
    pub session_id: String,
    /// Agent identifier
    pub agent_id: AgentId,
    /// Token expiration timestamp
    pub expires_at: SystemTime,
    /// Token signature
    pub signature: String,
}

impl SessionToken {
    /// Create a new session token
    pub fn new(session_id: String, agent_id: AgentId, expires_at: SystemTime) -> Self {
        let signature = Self::generate_signature(&session_id, &agent_id, expires_at);
        SessionToken {
            session_id,
            agent_id,
            expires_at,
            signature,
        }
    }

    /// Generate a signature for the session token
    fn generate_signature(session_id: &str, agent_id: &AgentId, expires_at: SystemTime) -> String {
        let mut hasher = Hasher::new();
        hasher.update(session_id.as_bytes());
        hasher.update(&agent_id.0);
        hasher.update(
            &expires_at
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_be_bytes(),
        );

        let hash = hasher.finalize();
        hex::encode(hash.as_bytes())
    }

    /// Verify the token signature
    pub fn verify_signature(&self) -> bool {
        let expected_signature =
            Self::generate_signature(&self.session_id, &self.agent_id, self.expires_at);
        self.signature == expected_signature
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        SystemTime::now() > self.expires_at
    }

    /// Check if token is valid
    pub fn is_valid(&self) -> bool {
        !self.is_expired() && self.verify_signature()
    }
}

/// Agent authentication manager
#[derive(Clone)]
pub struct AgentAuth {
    secp: Secp256k1<secp256k1::All>,
}

impl AgentAuth {
    /// Create a new authentication manager
    pub fn new() -> Self {
        let secp = Secp256k1::new();
        AgentAuth { secp }
    }

    /// Create a session token for an agent
    pub fn create_session_token(
        &self,
        session_id: String,
        agent_id: AgentId,
        expires_in: Duration,
    ) -> SessionToken {
        let expires_at = SystemTime::now() + expires_in;
        SessionToken::new(session_id, agent_id, expires_at)
    }

    /// Verify a session token
    pub fn verify_session_token(&self, token: &SessionToken) -> AgentResult<()> {
        if token.is_expired() {
            return Err(AgentError::SessionExpired(token.session_id.clone()));
        }

        if !token.verify_signature() {
            return Err(AgentError::InvalidSessionToken);
        }

        Ok(())
    }

    /// Generate a nonce for agent authentication
    pub fn generate_nonce(&self) -> String {
        let mut bytes = [0u8; 32];
        // In a real implementation, this would use a secure RNG
        // For now, we'll use a simple approach for demonstration
        bytes[0..8].copy_from_slice(
            &SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_be_bytes(),
        );
        // Add a counter to ensure uniqueness within the same second
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        bytes[8..16].copy_from_slice(&counter.to_be_bytes());
        hex::encode(bytes)
    }

    /// Verify agent signature
    pub fn verify_agent_signature(
        &self,
        _agent_id: &AgentId,
        _message: &[u8],
        signature: &[u8],
    ) -> AgentResult<()> {
        // In a real implementation, this would verify the signature
        // against the agent's public key
        // For now, we'll just check the length
        if signature.len() != 65 {
            // Standard secp256k1 signature length
            return Err(AgentError::InvalidSignature);
        }

        // TODO: Implement actual signature verification using agent_id as public key
        Ok(())
    }

    /// Create an authentication challenge
    pub fn create_challenge(&self) -> String {
        let nonce = self.generate_nonce();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        format!("{}:{}", nonce, timestamp)
    }
}

/// Authentication middleware for HTTP requests
#[derive(Clone)]
pub struct AuthMiddleware {
    auth: AgentAuth,
}

impl AuthMiddleware {
    /// Create a new authentication middleware
    pub fn new() -> Self {
        AuthMiddleware {
            auth: AgentAuth::new(),
        }
    }

    /// Get a reference to the inner AgentAuth
    pub fn auth(&self) -> &AgentAuth {
        &self.auth
    }

    /// Extract and validate session token from HTTP headers
    pub fn extract_session_token(
        &self,
        headers: &axum::http::HeaderMap,
    ) -> AgentResult<SessionToken> {
        // Extract Authorization header
        let auth_header = headers.get("Authorization").ok_or_else(|| {
            AgentError::AuthenticationFailed("Missing Authorization header".to_string())
        })?;

        let auth_str = auth_header.to_str().map_err(|_| {
            AgentError::AuthenticationFailed("Invalid Authorization header".to_string())
        })?;

        // Parse Bearer token
        if !auth_str.starts_with("Bearer ") {
            return Err(AgentError::AuthenticationFailed(
                "Invalid token format".to_string(),
            ));
        }

        let token_str = &auth_str[7..]; // Remove "Bearer "
        let token: SessionToken = serde_json::from_str(token_str)
            .map_err(|_| AgentError::AuthenticationFailed("Invalid token format".to_string()))?;

        // Verify token
        self.auth.verify_session_token(&token)?;

        Ok(token)
    }

    /// Create a rate limit key for session-based rate limiting
    pub fn get_rate_limit_key(&self, session_id: &str) -> String {
        format!("rate_limit:{}", session_id)
    }

    /// Create a permission check middleware
    pub fn require_permission(
        &self,
        required_permission: &str,
    ) -> impl Fn() -> Result<(), AgentError> {
        let _required_permission = required_permission.to_string();
        move || {
            // In a real implementation, this would check the session permissions
            // For now, we'll just allow everything
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_token_creation() {
        let auth = AgentAuth::new();
        let session_id = "test_session".to_string();
        let agent_id = AgentId([0u8; 33]);
        let expires_in = Duration::from_secs(3600);

        let token = auth.create_session_token(session_id, agent_id, expires_in);

        assert!(!token.session_id.is_empty());
        assert!(!token.is_expired());
        assert!(token.verify_signature());
    }

    #[test]
    fn test_session_token_expiration() {
        let auth = AgentAuth::new();
        let session_id = "test_session".to_string();
        let agent_id = AgentId([0u8; 33]);
        let expires_in = Duration::from_millis(100); // Very short expiration

        let token = auth.create_session_token(session_id, agent_id, expires_in);

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(200));

        assert!(token.is_expired());
        assert!(auth.verify_session_token(&token).is_err());
    }

    #[test]
    fn test_nonce_generation() {
        let auth = AgentAuth::new();
        let nonce1 = auth.generate_nonce();
        let nonce2 = auth.generate_nonce();

        // Nonces should be different
        assert_ne!(nonce1, nonce2);

        // Nonces should be valid hex
        assert!(hex::decode(&nonce1).is_ok());
        assert!(hex::decode(&nonce2).is_ok());
    }

    #[test]
    fn test_challenge_creation() {
        let auth = AgentAuth::new();
        let challenge = auth.create_challenge();

        // Challenge should contain nonce and timestamp
        assert!(challenge.contains(':'));

        let parts: Vec<&str> = challenge.split(':').collect();
        assert_eq!(parts.len(), 2);

        // Both parts should be valid hex
        assert!(hex::decode(parts[0]).is_ok());
        assert!(parts[1].parse::<u64>().is_ok());
    }

    #[test]
    fn test_auth_middleware() {
        let middleware = AuthMiddleware::new();
        let mut headers = http::HeaderMap::new();

        // Test missing header
        assert!(middleware.extract_session_token(&headers).is_err());

        // Test invalid header format
        headers.insert("Authorization", "Invalid".parse().unwrap());
        assert!(middleware.extract_session_token(&headers).is_err());

        // Test valid token (would need proper serialization in real implementation)
        // This test is simplified for demonstration
    }
}
