//! Agent_* JSON-RPC methods implementation

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::session::SessionPermission;
use crate::{
    error::{AgentError, AgentResult},
    session::AgentSession,
};
use strangecoin_core::types::Transaction;

/// Method registry for agent_* methods
pub struct MethodRegistry {
    methods: HashMap<String, Box<dyn AgentMethod + Send + Sync>>,
}

impl MethodRegistry {
    /// Create a new method registry
    pub fn new() -> Self {
        MethodRegistry {
            methods: HashMap::new(),
        }
    }

    /// Register all agent methods
    pub async fn register_all_methods(
        &mut self,
        session_manager: &Arc<crate::session::AgentSessionManager>,
        auth: &Arc<crate::auth::AuthMiddleware>,
    ) -> AgentResult<()> {
        // Register core session methods
        self.register_method(
            "agent_getSession",
            Box::new(SessionMethod::new(session_manager.clone())),
        );
        self.register_method(
            "agent_getSessionInfo",
            Box::new(SessionInfoMethod::new(session_manager.clone())),
        );
        self.register_method(
            "agent_closeSession",
            Box::new(CloseSessionMethod::new(session_manager.clone())),
        );

        // Register transaction batch methods
        self.register_method(
            "agent_signAndSubmitBatch",
            Box::new(SignAndSubmitBatchMethod::new(session_manager.clone())),
        );
        self.register_method(
            "agent_getBatchStatus",
            Box::new(GetBatchStatusMethod::new(session_manager.clone())),
        );
        self.register_method(
            "agent_getBatchReceipts",
            Box::new(GetBatchReceiptsMethod::new(session_manager.clone())),
        );

        // Register agent management methods
        self.register_method(
            "agent_getActivity",
            Box::new(GetActivityMethod::new(session_manager.clone())),
        );
        self.register_method(
            "agent_getBalance",
            Box::new(GetBalanceMethod::new(session_manager.clone())),
        );
        self.register_method(
            "agent_getNonce",
            Box::new(GetNonceMethod::new(session_manager.clone())),
        );

        // Register sponsorship methods
        self.register_method(
            "agent_sponsorTransaction",
            Box::new(SponsorTransactionMethod::new(session_manager.clone())),
        );
        self.register_method(
            "agent_getSponsorships",
            Box::new(GetSponsorshipsMethod::new(session_manager.clone())),
        );
        self.register_method(
            "agent_revokeSponsorship",
            Box::new(RevokeSponsorshipMethod::new(session_manager.clone())),
        );

        Ok(())
    }

    /// Register a single method
    pub fn register_method(&mut self, name: &str, method: Box<dyn AgentMethod + Send + Sync>) {
        self.methods.insert(name.to_string(), method);
    }

    /// Whether no methods are registered
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty()
    }

    /// Whether a method with the given name is registered
    pub fn contains(&self, name: &str) -> bool {
        self.methods.contains_key(name)
    }

    /// Number of registered methods
    pub fn len(&self) -> usize {
        self.methods.len()
    }

    /// Execute a method
    pub async fn execute_method(
        &self,
        method_name: &str,
        session: &AgentSession,
        params: Option<Value>,
    ) -> AgentResult<Value> {
        let method = self
            .methods
            .get(method_name)
            .ok_or_else(|| AgentError::MethodNotFound(method_name.to_string()))?;

        // Check permissions
        if !self.check_method_permissions(method_name, &session.permissions) {
            return Err(AgentError::permission_denied(method_name));
        }

        method.execute(session, params).await
    }

    /// Check if method permissions are satisfied
    pub fn check_method_permissions(
        &self,
        method_name: &str,
        permissions: &[SessionPermission],
    ) -> bool {
        match method_name {
            "agent_getSession" => true, // No permissions required for basic session creation
            "agent_getSessionInfo" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::ReadAccount)),
            "agent_closeSession" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::ManageSessions)),
            "agent_signAndSubmitBatch" => permissions.iter().any(|p| {
                p.includes(&SessionPermission::SubmitTransaction)
                    || p.includes(&SessionPermission::ManageBatches)
            }),
            "agent_getBatchStatus" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::ReadAccount)),
            "agent_getBatchReceipts" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::ReadAccount)),
            "agent_getActivity" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::ReadActivity)),
            "agent_getBalance" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::ReadAccount)),
            "agent_getNonce" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::ReadAccount)),
            "agent_sponsorTransaction" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::SponsorTransactions)),
            "agent_getSponsorships" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::SponsorTransactions)),
            "agent_revokeSponsorship" => permissions
                .iter()
                .any(|p| p.includes(&SessionPermission::SponsorTransactions)),
            // Methods outside the built-in matrix (registered via the public
            // register_method) carry no hardcoded permission requirement —
            // all 12 agent_* methods from register_all_methods are covered
            // by the arms above, so this default only affects custom
            // registrations. Denying here would make register_method useless:
            // every custom method would be unexecutable.
            _ => true,
        }
    }
}

/// Trait for agent methods
#[async_trait::async_trait]
pub trait AgentMethod: Send + Sync {
    /// Execute the method
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value>;
}

/// Session management methods
struct SessionMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl SessionMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        SessionMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for SessionMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        // Return session information
        Ok(serde_json::json!({
            "session_id": session.session_id,
            "agent_id": hex::encode(&session.agent_id.0),
            "created_at": session.created_at,
            "expires_at": session.expires_at,
            "permissions": session.permissions,
            "metadata": session.metadata
        }))
    }
}

struct SessionInfoMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl SessionInfoMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        SessionInfoMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for SessionInfoMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        // Extended session information
        let now = chrono::Utc::now();
        let time_to_expiry = session.expires_at.signed_duration_since(now);

        Ok(serde_json::json!({
            "session_id": session.session_id,
            "agent_id": hex::encode(&session.agent_id.0),
            "created_at": session.created_at,
            "expires_at": session.expires_at,
            "time_to_expiry": time_to_expiry.num_seconds(),
            "last_activity": session.last_activity,
            "permissions": session.permissions,
            "metadata": session.metadata,
            "session_age": now.signed_duration_since(session.created_at).num_seconds()
        }))
    }
}

struct CloseSessionMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl CloseSessionMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        CloseSessionMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for CloseSessionMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        // Close the session
        self.session_manager
            .close_session(&session.session_id)
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "session_id": session.session_id,
            "message": "Session closed successfully"
        }))
    }
}

/// Transaction batch methods
struct SignAndSubmitBatchMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl SignAndSubmitBatchMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        SignAndSubmitBatchMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for SignAndSubmitBatchMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.ok_or_else(|| AgentError::invalid_parameters("Missing parameters"))?;

        let batch: BatchRequest = serde_json::from_value(params)
            .map_err(|_| AgentError::invalid_parameters("Invalid batch format"))?;

        // Validate batch size
        if batch.transactions.len() > session.metadata.max_batch_size {
            return Err(AgentError::invalid_parameters(&format!(
                "Batch size exceeds maximum allowed: {}",
                session.metadata.max_batch_size
            )));
        }

        // Process batch (simplified for demonstration)
        let batch_id = Uuid::new_v4().to_string();
        let mut results = Vec::new();

        for (index, tx) in batch.transactions.iter().enumerate() {
            // In a real implementation, this would:
            // 1. Validate the transaction
            // 2. Sign it with the agent's key
            // 3. Submit it to the mempool
            // 4. Return the result

            let result = serde_json::json!({
                "index": index,
                "txid": format!("0x{}", hex::encode(Uuid::new_v4().as_bytes())),
                "status": "pending",
                "error": None::<String>
            });

            results.push(result);
        }

        Ok(serde_json::json!({
            "batch_id": batch_id,
            "transactions": results,
            "total_count": results.len(),
            "status": "submitted"
        }))
    }
}

struct GetBatchStatusMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl GetBatchStatusMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        GetBatchStatusMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for GetBatchStatusMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.ok_or_else(|| AgentError::invalid_parameters("Missing parameters"))?;

        let req: BatchStatusRequest = serde_json::from_value(params)
            .map_err(|_| AgentError::invalid_parameters("Invalid batch status request"))?;

        // In a real implementation, this would query the blockchain for batch status
        // For now, return a mock response
        Ok(serde_json::json!({
            "batch_id": req.batch_id,
            "status": "completed",
            "processed_count": 10,
            "total_count": 10,
            "failed_count": 0,
            "created_at": "2024-01-01T00:00:00Z",
            "completed_at": "2024-01-01T00:00:05Z"
        }))
    }
}

struct GetBatchReceiptsMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl GetBatchReceiptsMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        GetBatchReceiptsMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for GetBatchReceiptsMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.ok_or_else(|| AgentError::invalid_parameters("Missing parameters"))?;

        let req: BatchReceiptsRequest = serde_json::from_value(params)
            .map_err(|_| AgentError::invalid_parameters("Invalid batch receipts request"))?;

        // In a real implementation, this would fetch receipts from the blockchain
        // For now, return mock receipts
        let receipts = vec![
            serde_json::json!({
                "txid": "0x1234567890abcdef",
                "status": "success",
                "gas_used": 21000,
                "logs": []
            }),
            serde_json::json!({
                "txid": "0xabcdef1234567890",
                "status": "failed",
                "gas_used": 0,
                "logs": [],
                "error": "Insufficient balance"
            }),
        ];

        Ok(serde_json::json!({
            "batch_id": req.batch_id,
            "receipts": receipts
        }))
    }
}

/// Agent management methods
struct GetActivityMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl GetActivityMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        GetActivityMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for GetActivityMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.unwrap_or(Value::Null);

        let req: ActivityRequest = serde_json::from_value(params).unwrap_or_default();

        // In a real implementation, this would query the blockchain for agent activity
        // For now, return mock activity data
        let activities = vec![
            serde_json::json!({
                "id": "act_001",
                "timestamp": "2024-01-01T00:00:00Z",
                "type": "transaction",
                "details": {
                    "txid": "0x1234567890abcdef",
                    "amount": "1000000",
                    "currency": "STR"
                }
            }),
            serde_json::json!({
                "id": "act_002",
                "timestamp": "2024-01-01T00:05:00Z",
                "type": "batch",
                "details": {
                    "batch_id": "batch_001",
                    "transaction_count": 5,
                    "total_amount": "5000000"
                }
            }),
        ];

        Ok(serde_json::json!({
            "activities": activities,
            "total_count": activities.len(),
            "has_more": false
        }))
    }
}

struct GetBalanceMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl GetBalanceMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        GetBalanceMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for GetBalanceMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.unwrap_or(Value::Null);

        let req: BalanceRequest = serde_json::from_value(params).unwrap_or_default();

        // In a real implementation, this would query the blockchain for balance
        // For now, return mock balance
        Ok(serde_json::json!({
            "address": req.address.unwrap_or_else(|| hex::encode(&session.agent_id.0)),
            "balance": "1000000000",
            "pending": "50000000",
            "currency": "STR"
        }))
    }
}

struct GetNonceMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl GetNonceMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        GetNonceMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for GetNonceMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.unwrap_or(Value::Null);

        let req: NonceRequest = serde_json::from_value(params).unwrap_or_default();

        // In a real implementation, this would query the blockchain for nonce
        // For now, return mock nonce
        Ok(serde_json::json!({
            "address": req.address.unwrap_or_else(|| hex::encode(&session.agent_id.0)),
            "nonce": 42
        }))
    }
}

/// Sponsorship methods
struct SponsorTransactionMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl SponsorTransactionMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        SponsorTransactionMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for SponsorTransactionMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.ok_or_else(|| AgentError::invalid_parameters("Missing parameters"))?;

        let req: SponsorRequest = serde_json::from_value(params)
            .map_err(|_| AgentError::invalid_parameters("Invalid sponsorship request"))?;

        // In a real implementation, this would:
        // 1. Validate the transaction
        // 2. Check agent has sufficient balance
        // 3. Sponsor the transaction
        // 4. Return the sponsored transaction details

        Ok(serde_json::json!({
            "sponsorship_id": "spon_001",
            "txid": "0x1234567890abcdef",
            "sponsor": hex::encode(&session.agent_id.0),
            "beneficiary": req.beneficiary,
            "amount": req.amount,
            "fee": "10000",
            "status": "pending",
            "created_at": "2024-01-01T00:00:00Z"
        }))
    }
}

struct GetSponsorshipsMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl GetSponsorshipsMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        GetSponsorshipsMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for GetSponsorshipsMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.unwrap_or(Value::Null);

        let req: SponsorshipListRequest = serde_json::from_value(params).unwrap_or_default();

        // In a real implementation, this would query for active sponsorships
        // For now, return mock sponsorships
        let sponsorships = vec![serde_json::json!({
            "sponsorship_id": "spon_001",
            "txid": "0x1234567890abcdef",
            "beneficiary": "0xabcdef1234567890",
            "amount": "1000000",
            "fee": "10000",
            "status": "active",
            "created_at": "2024-01-01T00:00:00Z"
        })];

        Ok(serde_json::json!({
            "sponsorships": sponsorships,
            "total_count": sponsorships.len(),
            "total_amount": "1000000",
            "total_fees": "10000"
        }))
    }
}

struct RevokeSponsorshipMethod {
    session_manager: Arc<crate::session::AgentSessionManager>,
}

impl RevokeSponsorshipMethod {
    fn new(session_manager: Arc<crate::session::AgentSessionManager>) -> Self {
        RevokeSponsorshipMethod { session_manager }
    }
}

#[async_trait::async_trait]
impl AgentMethod for RevokeSponsorshipMethod {
    async fn execute(&self, session: &AgentSession, params: Option<Value>) -> AgentResult<Value> {
        let params = params.ok_or_else(|| AgentError::invalid_parameters("Missing parameters"))?;

        let req: RevokeSponsorshipRequest = serde_json::from_value(params)
            .map_err(|_| AgentError::invalid_parameters("Invalid revocation request"))?;

        // In a real implementation, this would revoke the sponsorship
        // For now, return success response
        Ok(serde_json::json!({
            "success": true,
            "sponsorship_id": req.sponsorship_id,
            "message": "Sponsorship revoked successfully"
        }))
    }
}

/// Request/response types
#[derive(Debug, Deserialize)]
struct BatchRequest {
    transactions: Vec<Transaction>,
}

#[derive(Debug, Deserialize)]
struct BatchStatusRequest {
    batch_id: String,
}

#[derive(Debug, Deserialize)]
struct BatchReceiptsRequest {
    batch_id: String,
}

#[derive(Debug, Deserialize, Default)]
struct ActivityRequest {
    limit: Option<usize>,
    offset: Option<usize>,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct BalanceRequest {
    address: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct NonceRequest {
    address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SponsorRequest {
    beneficiary: String,
    amount: String,
    max_fee: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SponsorshipListRequest {
    limit: Option<usize>,
    offset: Option<usize>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RevokeSponsorshipRequest {
    sponsorship_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_method_registry() {
        let mut registry = MethodRegistry::new();
        let session_manager = Arc::new(crate::session::AgentSessionManager::new(
            Arc::new(crate::session::InMemorySessionStore::new()),
            crate::auth::AgentAuth::new(),
            Duration::from_secs(3600),
            10,
        ));
        let auth = Arc::new(crate::auth::AuthMiddleware::new());

        registry
            .register_all_methods(&session_manager, &auth)
            .await
            .unwrap();

        // Test method lookup
        assert!(registry.methods.contains_key("agent_getSession"));
        assert!(registry.methods.contains_key("agent_getBalance"));
        assert!(registry.methods.contains_key("agent_sponsorTransaction"));
    }

    #[tokio::test]
    async fn test_batch_request_parsing() {
        let batch_json = json!({
            "transactions": [
                {
                    "type": "transfer",
                    "sender": "0x1234567890abcdef",
                    "receiver": "0xabcdef1234567890",
                    "amount": 1000000,
                    "nonce": 1,
                    "fee": 100,
                    "chain_id": 3,
                    "signature": [0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef]
                }
            ]
        });

        let batch: BatchRequest = serde_json::from_value(batch_json).unwrap();
        assert_eq!(batch.transactions.len(), 1);
    }

    #[tokio::test]
    async fn test_permission_checking() {
        let mut registry = MethodRegistry::new();
        let session_manager = Arc::new(crate::session::AgentSessionManager::new(
            Arc::new(crate::session::InMemorySessionStore::new()),
            crate::auth::AgentAuth::new(),
            Duration::from_secs(3600),
            10,
        ));
        let auth = Arc::new(crate::auth::AuthMiddleware::new());

        registry
            .register_all_methods(&session_manager, &auth)
            .await
            .unwrap();

        // Test permission checking
        let permissions = vec![SessionPermission::FullAccess];
        assert!(registry.check_method_permissions("agent_getSession", &permissions));
        assert!(registry.check_method_permissions("agent_signAndSubmitBatch", &permissions));
        assert!(registry.check_method_permissions("agent_sponsorTransaction", &permissions));
    }
}
