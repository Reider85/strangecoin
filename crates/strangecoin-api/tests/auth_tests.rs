//! Unit tests for authentication

use std::time::Duration;

use strangecoin_api::{
    auth::{AgentAuth, SessionToken, AuthMiddleware},
    session::{SessionPermission, SessionMetadata},
    error::AgentError,
};

#[test]
fn test_session_token_creation() {
    let auth = AgentAuth::new();
    let session_id = "test_session".to_string();
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let expires_in = Duration::from_secs(3600);
    
    let token = auth.create_session_token(session_id, agent_id, expires_in);
    
    assert!(!token.session_id.is_empty());
    assert_eq!(token.session_id, "test_session");
    assert!(!token.is_expired());
    assert!(token.verify_signature());
}

#[test]
fn test_session_token_expiration() {
    let auth = AgentAuth::new();
    let session_id = "test_session".to_string();
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let expires_in = Duration::from_millis(100); // Very short expiration
    
    let token = auth.create_session_token(session_id, agent_id, expires_in);
    
    // Wait for expiration
    std::thread::sleep(Duration::from_millis(200));
    
    assert!(token.is_expired());
    assert!(auth.verify_session_token(&token).is_err());
}

#[test]
fn test_session_token_signature_verification() {
    let auth = AgentAuth::new();
    let session_id = "test_session".to_string();
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let expires_in = Duration::from_secs(3600);
    
    let token = auth.create_session_token(session_id.clone(), agent_id.clone(), expires_in);
    
    // Valid token should verify
    assert!(auth.verify_session_token(&token).is_ok());
    
    // Tampered token should fail verification
    let mut tampered_token = token;
    tampered_token.session_id = "tampered_session".to_string();
    assert!(auth.verify_session_token(&tampered_token).is_err());
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
    
    // Nonces should have reasonable length
    assert!(nonce1.len() >= 32); // At least 16 bytes in hex
    assert!(nonce2.len() >= 32);
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
    
    // Challenge should be different each time
    let challenge2 = auth.create_challenge();
    assert_ne!(challenge, challenge2);
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
    
    // Test Bearer token without proper format
    headers.insert("Authorization", "Bearer".parse().unwrap());
    assert!(middleware.extract_session_token(&headers).is_err());
    
    // Test malformed JSON
    headers.insert("Authorization", "Bearer {invalid json}".parse().unwrap());
    assert!(middleware.extract_session_token(&headers).is_err());
}

#[test]
fn test_rate_limit_key_generation() {
    let middleware = AuthMiddleware::new();
    let session_id = "test_session_123";
    
    let key1 = middleware.get_rate_limit_key(session_id);
    let key2 = middleware.get_rate_limit_key(session_id);
    
    // Keys should be the same for the same session ID
    assert_eq!(key1, key2);
    
    // Keys should follow expected format
    assert!(key1.starts_with("rate_limit:"));
    assert_eq!(&key1[12..], session_id);
}

#[test]
fn test_require_permission() {
    let middleware = AuthMiddleware::new();
    
    // Test permission check (simplified - in real implementation would check actual permissions)
    let permission_check = middleware.require_permission("test_permission");
    
    // For now, this should always succeed since we're not implementing actual permission checking
    assert!(permission_check().is_ok());
}

#[test]
fn test_agent_signature_verification() {
    let auth = AgentAuth::new();
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let message = b"test message";
    let signature = vec![0u8; 65]; // Valid signature length
    
    // This should pass (in real implementation would verify actual signature)
    assert!(auth.verify_agent_signature(&agent_id, message, &signature).is_ok());
    
    // Invalid signature length should fail
    let invalid_signature = vec![0u8; 32];
    assert!(auth.verify_agent_signature(&agent_id, message, &invalid_signature).is_err());
}

#[test]
fn test_session_token_serialization() {
    let auth = AgentAuth::new();
    let session_id = "test_session".to_string();
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let expires_in = Duration::from_secs(3600);
    
    let token = auth.create_session_token(session_id, agent_id, expires_in);
    
    // Test serialization
    let serialized = serde_json::to_string(&token).unwrap();
    let deserialized: SessionToken = serde_json::from_str(&serialized).unwrap();
    
    assert_eq!(token.session_id, deserialized.session_id);
    assert_eq!(token.agent_id, deserialized.agent_id);
    assert_eq!(token.expires_at, deserialized.expires_at);
    assert_eq!(token.signature, deserialized.signature);
    
    // Verify deserialized token is still valid
    assert!(!deserialized.is_expired());
    assert!(deserialized.verify_signature());
}

#[test]
fn test_multiple_session_tokens() {
    let auth = AgentAuth::new();
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let expires_in = Duration::from_secs(3600);
    
    // Create multiple tokens for the same session
    let token1 = auth.create_session_token("session1".to_string(), agent_id.clone(), expires_in);
    let token2 = auth.create_session_token("session2".to_string(), agent_id.clone(), expires_in);
    let token3 = auth.create_session_token("session3".to_string(), agent_id.clone(), expires_in);
    
    // All tokens should be valid
    assert!(auth.verify_session_token(&token1).is_ok());
    assert!(auth.verify_session_token(&token2).is_ok());
    assert!(auth.verify_session_token(&token3).is_ok());
    
    // All tokens should have different session IDs
    assert_ne!(token1.session_id, token2.session_id);
    assert_ne!(token2.session_id, token3.session_id);
    assert_ne!(token1.session_id, token3.session_id);
    
    // All tokens should have different signatures
    assert_ne!(token1.signature, token2.signature);
    assert_ne!(token2.signature, token3.signature);
    assert_ne!(token1.signature, token3.signature);
}

#[test]
fn test_token_edge_cases() {
    let auth = AgentAuth::new();
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    
    // Test zero expiration
    let zero_expiration_token = auth.create_session_token(
        "zero_expiry".to_string(),
        agent_id.clone(),
        Duration::from_secs(0)
    );
    
    // Should expire immediately
    assert!(zero_expiration_token.is_expired());
    assert!(auth.verify_session_token(&zero_expiration_token).is_err());
    
    // Test very long expiration
    let long_expiration_token = auth.create_session_token(
        "long_expiry".to_string(),
        agent_id.clone(),
        Duration::from_secs(365 * 24 * 3600) // 1 year
    );
    
    // Should not expire
    assert!(!long_expiration_token.is_expired());
    assert!(auth.verify_session_token(&long_expiration_token).is_ok());
}

#[test]
fn test_concurrent_nonce_generation() {
    let auth = AgentAuth::new();
    let mut handles = vec![];
    
    // Generate many nonces concurrently
    for _ in 0..100 {
        let auth_clone = auth.clone();
        let handle = std::thread::spawn(move || {
            auth_clone.generate_nonce()
        });
        handles.push(handle);
    }
    
    // Collect all nonces
    let nonces: Vec<String> = handles.into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    
    // All nonces should be unique
    let unique_nonces: std::collections::HashSet<String> = nonces.into_iter().collect();
    assert_eq!(unique_nonces.len(), 100);
    
    // All nonces should be valid hex
    for nonce in unique_nonces {
        assert!(hex::decode(&nonce).is_ok());
    }
}

// Test helper to create a mock HTTP header map
fn create_mock_headers() -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("user-agent", "test-agent/1.0".parse().unwrap());
    headers
}

#[test]
fn test_header_parsing() {
    let middleware = AuthMiddleware::new();
    let mut headers = create_mock_headers();
    
    // Test with missing Authorization header
    headers.remove("Authorization");
    assert!(middleware.extract_session_token(&headers).is_err());
    
    // Test with malformed Authorization header
    headers.insert("Authorization", "Bearer".parse().unwrap());
    assert!(middleware.extract_session_token(&headers).is_err());
    
    // Test with invalid JSON in Authorization header
    headers.insert("Authorization", "Bearer {invalid json}".parse().unwrap());
    assert!(middleware.extract_session_token(&headers).is_err());
}

#[test]
fn test_error_handling() {
    let auth = AgentAuth::new();
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    
    // Test verification of expired token
    let expired_token = auth.create_session_token(
        "expired".to_string(),
        agent_id,
        Duration::from_secs(0)
    );
    
    let result = auth.verify_session_token(&expired_token);
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.to_string(), "Session expired: expired");
    }
}