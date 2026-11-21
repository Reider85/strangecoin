//! JSON-RPC server for agent operations

use axum::{
    extract::State,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

use crate::methods::MethodRegistry;
use crate::{auth::AuthMiddleware, error::AgentResult, session::AgentSessionManager};

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Listen address
    pub listen_addr: SocketAddr,
    /// Session timeout in seconds
    pub session_timeout: u64,
    /// Maximum sessions per agent
    pub max_sessions_per_agent: usize,
    /// Enable CORS
    pub enable_cors: bool,
    /// Enable rate limiting
    pub enable_rate_limit: bool,
    /// Rate limit requests per minute per session
    pub rate_limit_per_minute: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            listen_addr: "127.0.0.1:9002".parse().unwrap(),
            session_timeout: 3600, // 1 hour
            max_sessions_per_agent: 10,
            enable_cors: true,
            enable_rate_limit: true,
            rate_limit_per_minute: 60,
        }
    }
}

/// JSON-RPC request
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Request ID
    pub id: serde_json::Value,
    /// Method name
    pub method: String,
    /// Method parameters
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC response
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Response ID
    pub id: serde_json::Value,
    /// Result or error
    pub result: Option<serde_json::Value>,
    /// Error if any
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Error data (optional)
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    /// Create a successful response
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(id: serde_json::Value, code: i32, message: String) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}

/// Server state
#[derive(Clone)]
pub struct ServerState {
    /// Session manager
    pub session_manager: Arc<AgentSessionManager>,
    /// Authentication middleware
    pub auth: Arc<AuthMiddleware>,
    /// Method registry
    pub methods: Arc<MethodRegistry>,
    /// Rate limiter (could be implemented using Redis or in-memory)
    pub rate_limiter: Option<Arc<RateLimiter>>,
    /// Server configuration
    pub config: Arc<ServerConfig>,
}

/// Simple rate limiter for demonstration
#[derive(Clone)]
pub struct RateLimiter {
    requests: Arc<tokio::sync::RwLock<HashMap<String, Vec<u64>>>>,
    max_requests_per_minute: usize,
}

impl RateLimiter {
    pub fn new(max_requests_per_minute: usize) -> Self {
        RateLimiter {
            requests: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            max_requests_per_minute,
        }
    }

    pub async fn check_rate_limit(&self, session_id: &str) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut requests = self.requests.write().await;
        let session_requests = requests
            .entry(session_id.to_string())
            .or_insert_with(Vec::new);

        // Remove old requests (older than 1 minute)
        session_requests.retain(|&timestamp| now - timestamp < 60);

        // Check if we can add a new request
        if session_requests.len() < self.max_requests_per_minute {
            session_requests.push(now);
            true
        } else {
            false
        }
    }
}

/// Agent API server
pub struct AgentApiServer {
    config: ServerConfig,
    state: ServerState,
}

impl AgentApiServer {
    /// Create a new agent API server
    pub async fn new(config: ServerConfig) -> AgentResult<Self> {
        // Initialize session store
        let session_store = Arc::new(crate::session::InMemorySessionStore::new());

        // Initialize authentication
        let auth = Arc::new(AuthMiddleware::new());

        // Initialize session manager
        let session_manager = Arc::new(AgentSessionManager::new(
            session_store,
            auth.auth().clone(),
            Duration::from_secs(config.session_timeout),
            config.max_sessions_per_agent,
        ));

        // Initialize method registry
        let mut methods = MethodRegistry::new();
        methods
            .register_all_methods(&session_manager, &auth)
            .await?;
        let methods = Arc::new(methods);

        // Initialize rate limiter
        let rate_limiter = if config.enable_rate_limit {
            Some(Arc::new(RateLimiter::new(config.rate_limit_per_minute)))
        } else {
            None
        };

        let state = ServerState {
            session_manager,
            auth,
            methods,
            rate_limiter,
            config: Arc::new(config.clone()),
        };

        Ok(AgentApiServer { config, state })
    }

    /// Start the server
    pub async fn serve(self) -> AgentResult<()> {
        let app = Router::new()
            .route("/rpc", post(handle_json_rpc))
            .route("/health", get(handle_health))
            .with_state(self.state.clone())
            .layer(if self.config.enable_cors {
                CorsLayer::permissive()
            } else {
                tower_http::cors::CorsLayer::new()
            });

        let listener = TcpListener::bind(self.config.listen_addr)
            .await
            .map_err(|e| {
                crate::error::AgentError::InternalError(format!(
                    "Failed to bind to {}: {}",
                    self.config.listen_addr, e
                ))
            })?;

        info!("Agent API server listening on {}", self.config.listen_addr);

        axum::serve(listener, app)
            .await
            .map_err(|e| crate::error::AgentError::InternalError(format!("Server error: {}", e)))?;

        Ok(())
    }

    /// Get server state for testing
    pub fn state(&self) -> &ServerState {
        &self.state
    }
}

/// Handle JSON-RPC requests
pub async fn handle_json_rpc(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    info!("JSON-RPC request: {} {:?}", request.method, request.params);

    // Extract session token from headers
    let session = match state.auth.extract_session_token(&headers) {
        Ok(session) => session,
        Err(e) => {
            warn!("Authentication failed: {}", e);
            return Json(JsonRpcResponse::error(
                request.id,
                e.json_rpc_code(),
                e.to_string(),
            ));
        }
    };

    // Check rate limit
    if state.config.enable_rate_limit {
        if let Some(limiter) = &state.rate_limiter {
            if !limiter.check_rate_limit(&session.session_id).await {
                return Json(JsonRpcResponse::error(
                    request.id,
                    crate::error::AgentError::RateLimitExceeded.json_rpc_code(),
                    "Rate limit exceeded".to_string(),
                ));
            }
        }
    }

    // Get the actual session from the session manager
    let actual_session = match state.session_manager.get_session(&session.session_id).await {
        Ok(session) => session,
        Err(e) => {
            return Json(JsonRpcResponse::error(
                request.id,
                crate::error::AgentError::SessionNotFound("Session not found".to_string())
                    .json_rpc_code(),
                format!("Session not found: {}", e),
            ));
        }
    };

    // Find and execute the method
    match state
        .methods
        .execute_method(&request.method, &actual_session, request.params)
        .await
    {
        Ok(result) => Json(JsonRpcResponse::success(request.id, result)),
        Err(e) => {
            error!("Method execution failed: {}", e);
            Json(JsonRpcResponse::error(
                request.id,
                e.json_rpc_code(),
                e.to_string(),
            ))
        }
    }
}

/// Handle health check requests
async fn handle_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now(),
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// Batch JSON-RPC request
#[derive(Debug, Deserialize)]
pub struct BatchJsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

/// Batch JSON-RPC response
#[derive(Debug, Serialize)]
pub struct BatchJsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}

/// Handle batch JSON-RPC requests
pub async fn handle_batch_json_rpc(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(requests): Json<Vec<BatchJsonRpcRequest>>,
) -> Json<Vec<BatchJsonRpcResponse>> {
    let mut responses = Vec::new();

    // Extract session token once for all requests in the batch
    let session = match state.auth.extract_session_token(&headers) {
        Ok(session) => session,
        Err(e) => {
            // Return error for all requests in the batch
            for request in requests {
                responses.push(BatchJsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: e.json_rpc_code(),
                        message: e.to_string(),
                        data: None,
                    }),
                });
            }
            return Json(responses);
        }
    };

    // Process each request in the batch
    for request in requests {
        info!(
            "Batch JSON-RPC request: {} {:?}",
            request.method, request.params
        );

        // Check rate limit for each request
        if state.config.enable_rate_limit {
            if let Some(limiter) = &state.rate_limiter {
                if !limiter.check_rate_limit(&session.session_id).await {
                    responses.push(BatchJsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request.id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: crate::error::AgentError::RateLimitExceeded.json_rpc_code(),
                            message: "Rate limit exceeded".to_string(),
                            data: None,
                        }),
                    });
                    continue;
                }
            }
        }

        // Execute method
        // Get the actual session from the session manager
        let actual_session = match state.session_manager.get_session(&session.session_id).await {
            Ok(session) => session,
            Err(e) => {
                responses.push(BatchJsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: e.json_rpc_code(),
                        message: format!("Session not found: {}", e),
                        data: None,
                    }),
                });
                continue;
            }
        };

        match state
            .methods
            .execute_method(&request.method, &actual_session, request.params)
            .await
        {
            Ok(result) => {
                responses.push(BatchJsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: Some(result),
                    error: None,
                });
            }
            Err(e) => {
                error!("Batch method execution failed: {}", e);
                responses.push(BatchJsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: e.json_rpc_code(),
                        message: e.to_string(),
                        data: None,
                    }),
                });
            }
        }
    }

    Json(responses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_json_rpc_request_parsing() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "agent_getSession",
            "params": {}
        });

        let rpc_request: JsonRpcRequest = serde_json::from_value(request).unwrap();
        assert_eq!(rpc_request.jsonrpc, "2.0");
        assert_eq!(rpc_request.id, serde_json::Value::Number(1.into()));
        assert_eq!(rpc_request.method, "agent_getSession");
        assert_eq!(rpc_request.params, Some(json!({})));
    }

    #[tokio::test]
    async fn test_json_rpc_response_creation() {
        let response = JsonRpcResponse::success(
            serde_json::Value::Number(1.into()),
            json!({"session_id": "test"}),
        );

        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, serde_json::Value::Number(1.into()));
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_json_rpc_error_creation() {
        let response = JsonRpcResponse::error(
            serde_json::Value::Number(1.into()),
            -32001,
            "Session not found".to_string(),
        );

        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, serde_json::Value::Number(1.into()));
        assert!(response.result.is_none());
        assert!(response.error.is_some());
        assert_eq!(response.error.as_ref().unwrap().code, -32001);
        assert_eq!(
            response.error.as_ref().unwrap().message,
            "Session not found"
        );
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(3);

        // Should allow first 3 requests
        assert!(limiter.check_rate_limit("test").await);
        assert!(limiter.check_rate_limit("test").await);
        assert!(limiter.check_rate_limit("test").await);

        // Should reject 4th request
        assert!(!limiter.check_rate_limit("test").await);
    }
}
