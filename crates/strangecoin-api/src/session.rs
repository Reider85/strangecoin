//! Agent session management

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::auth::{AgentAuth, SessionToken};
use crate::error::{AgentError, AgentResult};
use strangecoin_core::types::AgentId;

/// Session permissions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionPermission {
    /// Read account information
    ReadAccount,
    /// Submit transactions
    SubmitTransaction,
    /// Create and manage batches
    ManageBatches,
    /// Sponsor transactions
    SponsorTransactions,
    /// Access activity feed
    ReadActivity,
    /// Manage sessions
    ManageSessions,
    /// All permissions
    FullAccess,
}

impl SessionPermission {
    /// Check if this permission includes another permission
    pub fn includes(&self, other: &SessionPermission) -> bool {
        match self {
            SessionPermission::FullAccess => true,
            _ => self == other,
        }
    }
}

/// Agent session data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    /// Unique session identifier
    pub session_id: String,
    /// Agent identifier
    pub agent_id: AgentId,
    /// Session creation time
    pub created_at: DateTime<Utc>,
    /// Session expiration time
    pub expires_at: DateTime<Utc>,
    /// Last activity time
    pub last_activity: DateTime<Utc>,
    /// Session permissions
    pub permissions: Vec<SessionPermission>,
    /// Session metadata
    pub metadata: SessionMetadata,
}

/// Session metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// IP address
    pub ip_address: Option<String>,
    /// User agent
    pub user_agent: Option<String>,
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Rate limit per minute
    pub rate_limit_per_minute: usize,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        SessionMetadata {
            ip_address: None,
            user_agent: None,
            max_batch_size: 100,
            rate_limit_per_minute: 60,
        }
    }
}

impl AgentSession {
    /// Create a new agent session
    pub fn new(
        session_id: String,
        agent_id: AgentId,
        permissions: Vec<SessionPermission>,
        expires_in: Duration,
        metadata: SessionMetadata,
    ) -> Self {
        let now = Utc::now();
        AgentSession {
            session_id,
            agent_id,
            created_at: now,
            expires_at: now + expires_in,
            last_activity: now,
            permissions,
            metadata,
        }
    }

    /// Check if session is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if session has a specific permission
    pub fn has_permission(&self, permission: &SessionPermission) -> bool {
        self.permissions.iter().any(|p| p.includes(permission))
    }

    /// Update last activity time
    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
    }

    /// Extend session expiration
    pub fn extend_expiration(&mut self, additional_time: Duration) {
        self.expires_at = Utc::now() + additional_time;
    }
}

/// Session store trait
#[async_trait::async_trait]
pub trait AgentSessionStore: Send + Sync {
    /// Store a session
    async fn store_session(&self, session: AgentSession) -> AgentResult<()>;
    /// Retrieve a session by ID
    async fn get_session(&self, session_id: &str) -> AgentResult<Option<AgentSession>>;
    /// Delete a session
    async fn delete_session(&self, session_id: &str) -> AgentResult<()>;
    /// List all sessions for an agent
    async fn list_sessions(&self, agent_id: &AgentId) -> AgentResult<Vec<AgentSession>>;
    /// Cleanup expired sessions
    async fn cleanup_expired_sessions(&self) -> AgentResult<usize>;
}

/// In-memory session store implementation
pub struct InMemorySessionStore {
    sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
}

impl InMemorySessionStore {
    /// Create a new in-memory session store
    pub fn new() -> Self {
        InMemorySessionStore {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl AgentSessionStore for InMemorySessionStore {
    async fn store_session(&self, session: AgentSession) -> AgentResult<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.session_id.clone(), session);
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> AgentResult<Option<AgentSession>> {
        let sessions = self.sessions.read().await;
        match sessions.get(session_id) {
            Some(session) => {
                // Check if session is expired
                if session.is_expired() {
                    return Err(AgentError::SessionExpired(session.session_id.clone()));
                }
                Ok(Some(session.clone()))
            }
            None => Ok(None),
        }
    }

    async fn delete_session(&self, session_id: &str) -> AgentResult<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }

    async fn list_sessions(&self, agent_id: &AgentId) -> AgentResult<Vec<AgentSession>> {
        let sessions = self.sessions.read().await;
        let sessions: Vec<AgentSession> = sessions
            .values()
            .filter(|s| s.agent_id == *agent_id && !s.is_expired())
            .cloned()
            .collect();
        Ok(sessions)
    }

    async fn cleanup_expired_sessions(&self) -> AgentResult<usize> {
        let mut sessions = self.sessions.write().await;
        let _now = Utc::now();
        let expired: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| session.is_expired())
            .map(|(id, _)| id.clone())
            .collect();

        for session_id in &expired {
            sessions.remove(session_id);
        }

        Ok(expired.len())
    }
}

/// Session manager for agent operations
pub struct AgentSessionManager {
    store: Arc<dyn AgentSessionStore>,
    auth: AgentAuth,
    default_session_ttl: Duration,
    max_sessions_per_agent: usize,
}

impl AgentSessionManager {
    /// Create a new session manager
    pub fn new(
        store: Arc<dyn AgentSessionStore>,
        auth: AgentAuth,
        default_session_ttl: Duration,
        max_sessions_per_agent: usize,
    ) -> Self {
        Self {
            store,
            auth,
            default_session_ttl,
            max_sessions_per_agent,
        }
    }

    /// Create a new session for an agent
    pub async fn create_session(
        &self,
        agent_id: AgentId,
        permissions: Vec<SessionPermission>,
        metadata: SessionMetadata,
    ) -> AgentResult<AgentSession> {
        // Check session limit
        let existing_sessions = self.store.list_sessions(&agent_id).await?;
        if existing_sessions.len() >= self.max_sessions_per_agent {
            return Err(AgentError::SessionLimitExceeded);
        }

        let session_id = self.generate_session_id();
        let session = AgentSession::new(
            session_id.clone(),
            agent_id,
            permissions,
            self.default_session_ttl,
            metadata,
        );

        self.store.store_session(session.clone()).await?;
        Ok(session)
    }

    /// Get a session by ID
    pub async fn get_session(&self, session_id: &str) -> AgentResult<AgentSession> {
        let session = self
            .store
            .get_session(session_id)
            .await?
            .ok_or_else(|| AgentError::session_not_found(session_id))?;
        Ok(session)
    }

    /// Validate a session token
    pub async fn validate_session_token(&self, token: &SessionToken) -> AgentResult<AgentSession> {
        let mut session = self.get_session(&token.session_id).await?;

        // Validate session ownership
        if session.agent_id != token.agent_id {
            return Err(AgentError::InvalidSessionToken);
        }

        // Update last activity
        session.update_activity();
        Ok(session)
    }

    /// List all sessions for an agent
    pub async fn list_sessions_for_agent(
        &self,
        agent_id: &AgentId,
    ) -> AgentResult<Vec<AgentSession>> {
        let sessions = self.store.list_sessions(agent_id).await?;
        Ok(sessions)
    }

    /// Close a session

    /// Close a session
    pub async fn close_session(&self, session_id: &str) -> AgentResult<()> {
        self.store.delete_session(session_id).await
    }

    /// Close all sessions for an agent
    pub async fn close_agent_sessions(&self, agent_id: &AgentId) -> AgentResult<usize> {
        let sessions = self.store.list_sessions(agent_id).await?;
        let count = sessions.len();

        for session in sessions {
            self.store.delete_session(&session.session_id).await?;
        }

        Ok(count)
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self) -> AgentResult<usize> {
        self.store.cleanup_expired_sessions().await
    }

    /// Generate a unique session ID
    fn generate_session_id(&self) -> String {
        let uuid = Uuid::new_v4();
        format!("agent_{}", uuid)
    }

    /// Extend session expiration
    pub async fn extend_session(
        &self,
        session_id: &str,
        additional_time: Duration,
    ) -> AgentResult<()> {
        let mut session = self.get_session(session_id).await?;
        session.extend_expiration(additional_time);
        self.store.store_session(session).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_creation() {
        let store = Arc::new(InMemorySessionStore::new());
        let auth = AgentAuth::new();
        let manager = AgentSessionManager::new(store, auth, Duration::from_secs(3600), 10);

        let agent_id = AgentId([0u8; 33]); // Dummy agent ID
        let permissions = vec![SessionPermission::FullAccess];
        let metadata = SessionMetadata::default();

        let session = manager
            .create_session(agent_id, permissions, metadata)
            .await
            .unwrap();
        assert!(!session.session_id.is_empty());
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

        let agent_id = AgentId([0u8; 33]);
        let permissions = vec![SessionPermission::FullAccess];
        let metadata = SessionMetadata::default();

        let session = manager
            .create_session(agent_id.clone(), permissions, metadata)
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(200)).await;

        let result = manager.get_session(&session.session_id).await;
        assert!(result.is_err());
        if let Err(AgentError::SessionExpired(_)) = result {
            // Expected
        } else {
            panic!("Expected SessionExpired error");
        }
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

        let agent_id = AgentId([0u8; 33]);
        let permissions = vec![SessionPermission::FullAccess];
        let metadata = SessionMetadata::default();

        // Create first session
        manager
            .create_session(agent_id.clone(), permissions.clone(), metadata.clone())
            .await
            .unwrap();

        // Create second session
        manager
            .create_session(agent_id.clone(), permissions.clone(), metadata.clone())
            .await
            .unwrap();

        // Try to create third session - should fail
        let result = manager
            .create_session(agent_id, permissions, metadata)
            .await;
        assert!(result.is_err());
        if let Err(AgentError::SessionLimitExceeded) = result {
            // Expected
        } else {
            panic!("Expected SessionLimitExceeded error");
        }
    }
}
