//! Unit tests for agent methods

use std::sync::Arc;
use std::time::Duration;
use serde_json::json;

use strangecoin_api::{
    session::{AgentSession, AgentSessionManager, InMemorySessionStore, SessionPermission, SessionMetadata},
    auth::AgentAuth,
    methods::{MethodRegistry, AgentMethod},
    error::{AgentError, AgentResult},
};

#[tokio::test]
async fn test_method_registry_creation() {
    let mut registry = MethodRegistry::new();
    assert!(registry.methods.is_empty());
    
    // Test method registration
    registry.register_method("test_method", Box::new(TestMethod::new()));
    assert!(registry.methods.contains_key("test_method"));
    assert_eq!(registry.methods.len(), 1);
}

#[tokio::test]
async fn test_method_execution() {
    let mut registry = MethodRegistry::new();
    
    // Register test method
    registry.register_method("test_method", Box::new(TestMethod::new()));
    
    // Create test session
    let session = create_test_session().await;
    
    // Execute method
    let result = registry.execute_method("test_method", &session, Some(json!({"test": "value"}))).await;
    
    assert!(result.is_ok());
    let result_value = result.unwrap();
    assert_eq!(result_value["test"], "value");
    assert_eq!(result_value["session_id"], session.session_id);
}

#[tokio::test]
async fn test_method_not_found() {
    let mut registry = MethodRegistry::new();
    let session = create_test_session().await;
    
    // Try to execute non-existent method
    let result = registry.execute_method("non_existent_method", &session, Some(json!({}))).await;
    
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.to_string(), "Method not found: non_existent_method");
    }
}

#[tokio::test]
async fn test_method_permission_checking() {
    let mut registry = MethodRegistry::new();
    
    // Register methods with different permission requirements
    registry.register_method("read_method", Box::new(TestMethod::new()));
    registry.register_method("write_method", Box::new(TestMethod::new()));
    
    // Test with read-only session
    let read_session = create_test_session_with_permissions(vec![SessionPermission::ReadAccount]).await;
    assert!(registry.check_method_permissions("read_method", &read_session.permissions));
    assert!(!registry.check_method_permissions("write_method", &read_session.permissions));
    
    // Test with full access session
    let full_session = create_test_session_with_permissions(vec![SessionPermission::FullAccess]).await;
    assert!(registry.check_method_permissions("read_method", &full_session.permissions));
    assert!(registry.check_method_permissions("write_method", &full_session.permissions));
    
    // Test with empty permissions
    let empty_session = create_test_session_with_permissions(vec![]).await;
    assert!(!registry.check_method_permissions("read_method", &empty_session.permissions));
    assert!(!registry.check_method_permissions("write_method", &empty_session.permissions));
}

#[tokio::test]
async fn test_method_parameter_validation() {
    let mut registry = MethodRegistry::new();
    
    // Register method that requires parameters
    registry.register_method("required_params_method", Box::new(RequiredParamsMethod::new()));
    
    let session = create_test_session().await;
    
    // Test with missing parameters
    let result = registry.execute_method("required_params_method", &session, None).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("Missing parameters"));
    }
    
    // Test with invalid parameters
    let result = registry.execute_method("required_params_method", &session, Some(json!({"invalid": "params"}))).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("Invalid parameters"));
    }
    
    // Test with valid parameters
    let result = registry.execute_method("required_params_method", &session, Some(json!({"valid": "parameters"}))).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_batch_method_execution() {
    let mut registry = MethodRegistry::new();
    
    // Register batch method
    registry.register_method("batch_method", Box::new(BatchMethod::new()));
    
    let session = create_test_session().await;
    
    // Test batch execution
    let batch_params = json!({
        "items": ["item1", "item2", "item3"]
    });
    
    let result = registry.execute_method("batch_method", &session, Some(batch_params)).await;
    assert!(result.is_ok());
    
    let result_value = result.unwrap();
    assert!(result_value["results"].is_array());
    assert_eq!(result_value["results"].as_array().unwrap().len(), 3);
    assert_eq!(result_value["count"], 3);
}

#[tokio::test]
async fn test_error_handling_in_methods() {
    let mut registry = MethodRegistry::new();
    
    // Register method that returns errors
    registry.register_method("error_method", Box::new(ErrorMethod::new()));
    
    let session = create_test_session().await;
    
    // Test error execution
    let result = registry.execute_method("error_method", &session, Some(json!({}))).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.to_string(), "Method error: Something went wrong");
    }
}

#[tokio::test]
async fn test_method_state_isolation() {
    let mut registry = MethodRegistry::new();
    
    // Register stateful method
    registry.register_method("stateful_method", Box::new(StatefulMethod::new()));
    
    let session1 = create_test_session().await;
    let session2 = create_test_session().await;
    
    // Execute method with different sessions
    let result1 = registry.execute_method("stateful_method", &session1, Some(json!({"action": "increment"}))).await;
    let result2 = registry.execute_method("stateful_method", &session2, Some(json!({"action": "increment"}))).await;
    
    // Both should succeed but have different session IDs
    assert!(result1.is_ok());
    assert!(result2.is_ok());
    assert_ne!(result1.unwrap()["session_id"], result2.unwrap()["session_id"]);
}

#[tokio::test]
async fn test_concurrent_method_execution() {
    let mut registry = MethodRegistry::new();
    
    // Register thread-safe method
    registry.register_method("concurrent_method", Box::new(ConcurrentMethod::new()));
    
    let session = create_test_session().await;
    
    // Execute method concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let registry_clone = registry.clone();
        let session_clone = session.clone();
        
        let handle = tokio::spawn(async move {
            let params = json!({"iteration": i});
            registry_clone.execute_method("concurrent_method", &session_clone, Some(params)).await
        });
        
        handles.push(handle);
    }
    
    // Wait for all executions to complete
    let mut results = Vec::new();
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        results.push(result.unwrap());
    }
    
    // All results should be valid
    assert_eq!(results.len(), 10);
    
    // Each result should contain the iteration number
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result["iteration"], i);
    }
}

// Helper function to create a test session
async fn create_test_session() -> AgentSession {
    create_test_session_with_permissions(vec![SessionPermission::FullAccess]).await
}

async fn create_test_session_with_permissions(permissions: Vec<SessionPermission>) -> AgentSession {
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let manager = AgentSessionManager::new(
        store,
        auth,
        std::time::Duration::from_secs(3600),
        10,
    );

    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let metadata = SessionMetadata::default();

    manager.create_session(agent_id, permissions, metadata).await.unwrap()
}

// Test method implementations
struct TestMethod {
    greeting: String,
}

impl TestMethod {
    fn new() -> Self {
        TestMethod {
            greeting: "Hello".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl AgentMethod for TestMethod {
    async fn execute(&self, session: &AgentSession, params: Option<serde_json::Value>) -> AgentResult<serde_json::Value> {
        let params = params.unwrap_or(serde_json::Value::Null);
        
        Ok(serde_json::json!({
            "greeting": self.greeting,
            "session_id": session.session_id,
            "test_data": params
        }))
    }
}

struct RequiredParamsMethod {}

impl RequiredParamsMethod {
    fn new() -> Self {
        RequiredParamsMethod {}
    }
}

#[async_trait::async_trait]
impl AgentMethod for RequiredParamsMethod {
    async fn execute(&self, _session: &AgentSession, params: Option<serde_json::Value>) -> AgentResult<serde_json::Value> {
        let params = params.ok_or_else(|| crate::error::AgentError::invalid_parameters("Missing parameters"))?;
        
        // Check if required field exists
        if !params.is_object() || !params.get("valid").is_some() {
            return Err(crate::error::AgentError::invalid_parameters("Invalid parameters"));
        }
        
        Ok(serde_json::json!({
            "success": true,
            "valid": params["valid"]
        }))
    }
}

struct BatchMethod {}

impl BatchMethod {
    fn new() -> Self {
        BatchMethod {}
    }
}

#[async_trait::async_trait]
impl AgentMethod for BatchMethod {
    async fn execute(&self, _session: &AgentSession, params: Option<serde_json::Value>) -> AgentResult<serde_json::Value> {
        let params = params.ok_or_else(|| crate::error::AgentError::invalid_parameters("Missing parameters"))?;
        
        let items = params.get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| crate::error::AgentError::invalid_parameters("Invalid items format"))?;
        
        let mut results = Vec::new();
        for (index, item) in items.iter().enumerate() {
            results.push(serde_json::json!({
                "index": index,
                "item": item,
                "processed": true
            }));
        }
        
        Ok(serde_json::json!({
            "results": results,
            "count": results.len()
        }))
    }
}

struct ErrorMethod {}

impl ErrorMethod {
    fn new() -> Self {
        ErrorMethod {}
    }
}

#[async_trait::async_trait]
impl AgentMethod for ErrorMethod {
    async fn execute(&self, _session: &AgentSession, _params: Option<serde_json::Value>) -> AgentResult<serde_json::Value> {
        Err(crate::error::AgentError::InternalError("Something went wrong".to_string()))
    }
}

struct StatefulMethod {
    counter: std::sync::Arc<std::sync::Mutex<u64>>,
}

impl StatefulMethod {
    fn new() -> Self {
        StatefulMethod {
            counter: std::sync::Arc::new(std::sync::Mutex::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl AgentMethod for StatefulMethod {
    async fn execute(&self, session: &AgentSession, params: Option<serde_json::Value>) -> AgentResult<serde_json::Value> {
        let params = params.unwrap_or(serde_json::Value::Null);
        
        let mut counter = self.counter.lock().unwrap();
        if params.get("action").and_then(|v| v.as_str()) == Some("increment") {
            *counter += 1;
        }
        
        Ok(serde_json::json!({
            "session_id": session.session_id,
            "counter_value": *counter,
            "action": params.get("action")
        }))
    }
}

struct ConcurrentMethod {}

impl ConcurrentMethod {
    fn new() -> Self {
        ConcurrentMethod {}
    }
}

#[async_trait::async_trait]
impl AgentMethod for ConcurrentMethod {
    async fn execute(&self, session: &AgentSession, params: Option<serde_json::Value>) -> AgentResult<serde_json::Value> {
        let params = params.unwrap_or(serde_json::Value::Null);
        
        // Simulate some work
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        
        Ok(serde_json::json!({
            "session_id": session.session_id,
            "iteration": params.get("iteration").unwrap_or(&serde_json::Value::Null),
            "timestamp": chrono::Utc::now()
        }))
    }
}