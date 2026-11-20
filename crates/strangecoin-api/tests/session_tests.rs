//! Unit tests for session management

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use strangecoin_api::{
    session::{AgentSession, AgentSessionManager, InMemorySessionStore, SessionPermission, SessionMetadata},
    auth::AgentAuth,
    error::AgentResult,
};

#[tokio::test]
async fn test_session_creation() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    let session = manager.create_session(agent_id.clone(), permissions, metadata).await.unwrap();
    
    assert!(!session.session_id.is_empty());
    assert_eq!(session.agent_id, agent_id);
    assert_eq!(session.permissions.len(), 1);
    assert!(session.has_permission(&SessionPermission::FullAccess));
    assert!(!session.is_expired());
}

#[tokio::test]
async fn test_session_expiration() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_millis(100), // Very short TTL
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    let session = manager.create_session(agent_id.clone(), permissions, metadata).await.unwrap();
    
    // Wait for expiration
    sleep(Duration::from_millis(200)).await;
    
    let result = manager.get_session(&session.session_id).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.to_string(), "Session expired: agent_...");
    }
}

#[tokio::test]
async fn test_session_permissions() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    
    // Test with limited permissions
    let limited_permissions = vec![SessionPermission::ReadAccount];
    let metadata = SessionMetadata::default();
    
    let session = manager.create_session(agent_id.clone(), limited_permissions, metadata).await.unwrap();
    
    assert!(session.has_permission(&SessionPermission::ReadAccount));
    assert!(!session.has_permission(&SessionPermission::SubmitTransaction));
    assert!(!session.has_permission(&SessionPermission::FullAccess));
    
    // Test with full access
    let full_permissions = vec![SessionPermission::FullAccess];
    let full_session = manager.create_session(agent_id, full_permissions, metadata).await.unwrap();
    
    assert!(full_session.has_permission(&SessionPermission::ReadAccount));
    assert!(full_session.has_permission(&SessionPermission::SubmitTransaction));
    assert!(full_session.has_permission(&SessionPermission::FullAccess));
}

#[tokio::test]
async fn test_session_lifecycle() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    // Create session
    let session = manager.create_session(agent_id.clone(), permissions, metadata).await.unwrap();
    let session_id = session.session_id.clone();
    
    // Get session
    let retrieved_session = manager.get_session(&session_id).await.unwrap();
    assert_eq!(retrieved_session.session_id, session_id);
    
    // Update activity
    retrieved_session.update_activity();
    
    // Close session
    manager.close_session(&session_id).await.unwrap();
    
    // Verify session is closed
    let result = manager.get_session(&session_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_session_limit() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        2, // Very low limit
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    // Create first session
    let session1 = manager.create_session(agent_id.clone(), permissions.clone(), metadata.clone()).await.unwrap();
    
    // Create second session
    let session2 = manager.create_session(agent_id.clone(), permissions.clone(), metadata.clone()).await.unwrap();
    
    // Try to create third session - should fail
    let result = manager.create_session(agent_id, permissions, metadata).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.to_string(), "Session limit exceeded");
    }
    
    // Verify we have exactly 2 sessions
    let sessions = store.list_sessions(&agent_id).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_agent_session_cleanup() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    // Create multiple sessions
    let session1 = manager.create_session(agent_id.clone(), permissions.clone(), metadata.clone()).await.unwrap();
    let session2 = manager.create_session(agent_id.clone(), permissions.clone(), metadata.clone()).await.unwrap();
    let session3 = manager.create_session(agent_id.clone(), permissions.clone(), metadata.clone()).await.unwrap();
    
    // Verify all sessions exist
    let sessions = store.list_sessions(&agent_id).await.unwrap();
    assert_eq!(sessions.len(), 3);
    
    // Close all sessions for the agent
    let closed_count = manager.close_agent_sessions(&agent_id).await.unwrap();
    assert_eq!(closed_count, 3);
    
    // Verify all sessions are closed
    let sessions = store.list_sessions(&agent_id).await.unwrap();
    assert_eq!(sessions.len(), 0);
}

#[tokio::test]
async fn test_session_extension() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    let session = manager.create_session(agent_id.clone(), permissions, metadata).await.unwrap();
    let session_id = session.session_id.clone();
    
    // Wait a bit
    sleep(Duration::from_millis(100)).await;
    
    // Extend session
    let additional_time = Duration::from_secs(1800); // 30 more minutes
    manager.extend_session(&session_id, additional_time).await.unwrap();
    
    // Verify session is extended
    let extended_session = manager.get_session(&session_id).await.unwrap();
    assert!(!extended_session.is_expired());
    
    // The new expiration should be later than original
    assert!(extended_session.expires_at > session.expires_at);
}

#[tokio::test]
async fn test_session_metadata() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    
    let mut metadata = SessionMetadata::default();
    metadata.ip_address = Some("192.168.1.1".to_string());
    metadata.user_agent = Some("Test Agent/1.0".to_string());
    metadata.max_batch_size = 50;
    metadata.rate_limit_per_minute = 30;

    let session = manager.create_session(agent_id, permissions, metadata).await.unwrap();
    
    assert_eq!(session.metadata.ip_address, Some("192.168.1.1".to_string()));
    assert_eq!(session.metadata.user_agent, Some("Test Agent/1.0".to_string()));
    assert_eq!(session.metadata.max_batch_size, 50);
    assert_eq!(session.metadata.rate_limit_per_minute, 30);
}

#[tokio::test]
async fn test_session_store_operations() {
    let store = Arc::new(InMemorySessionStore::new());

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    // Create session
    let session = AgentSession::new(
        "test_session".to_string(),
        agent_id.clone(),
        permissions,
        Duration::from_secs(3600),
        metadata,
    );

    // Store session
    store.store_session(session.clone()).await.unwrap();

    // Retrieve session
    let retrieved_session = store.get_session("test_session").await.unwrap().unwrap();
    assert_eq!(retrieved_session.session_id, "test_session");
    assert_eq!(retrieved_session.agent_id, agent_id);

    // Delete session
    store.delete_session("test_session").await.unwrap();

    // Verify session is deleted
    let result = store.get_session("test_session").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_session_cleanup_expired() {
    let store = Arc::new(InMemorySessionStore::new());

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    // Create session with very short TTL
    let session1 = AgentSession::new(
        "session1".to_string(),
        agent_id.clone(),
        permissions.clone(),
        Duration::from_millis(100),
        metadata.clone(),
    );

    // Create another session with longer TTL
    let session2 = AgentSession::new(
        "session2".to_string(),
        agent_id.clone(),
        permissions,
        Duration::from_secs(3600),
        metadata,
    );

    // Store both sessions
    store.store_session(session1).await.unwrap();
    store.store_session(session2).await.unwrap();

    // Wait for first session to expire
    sleep(Duration::from_millis(200)).await;

    // Cleanup expired sessions
    let cleaned_count = store.cleanup_expired_sessions().await.unwrap();
    assert_eq!(cleaned_count, 1);

    // Verify only the second session remains
    let remaining_sessions = store.list_sessions(&agent_id).await.unwrap();
    assert_eq!(remaining_sessions.len(), 1);
    assert_eq!(remaining_sessions[0].session_id, "session2");
}

#[tokio::test]
async fn test_session_concurrent_access() {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![SessionPermission::FullAccess];
    let metadata = SessionMetadata::default();

    // Create session
    let session = manager.create_session(agent_id.clone(), permissions, metadata).await.unwrap();
    let session_id = session.session_id.clone();

    // Concurrent access test
    let mut handles = vec![];
    
    for i in 0..5 {
        let session_id_clone = session_id.clone();
        let store_clone = store.clone();
        
        let handle = tokio::spawn(async move {
            let session = store_clone.get_session(&session_id_clone).await.unwrap().unwrap();
            session.update_activity();
            session.session_id.clone()
        });
        
        handles.push(handle);
    }

    // Wait for all handles to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result, session_id);
    }

    // Verify session still exists and is valid
    let final_session = manager.get_session(&session_id).await.unwrap();
    assert_eq!(final_session.session_id, session_id);
    assert!(!final_session.is_expired());
}