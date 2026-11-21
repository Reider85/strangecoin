//! Integration tests for agent API

use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

use strangecoin_api::{AgentApiServer, ServerConfig};
use strangecoin_core::types::AgentId;

#[tokio::test]
async fn test_agent_api_server_startup() {
    let config = ServerConfig {
        listen_addr: "127.0.0.1:0".parse().unwrap(), // Use random port
        session_timeout: 3600,
        max_sessions_per_agent: 10,
        enable_cors: true,
        enable_rate_limit: true,
        rate_limit_per_minute: 60,
    };

    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    // Test that server state is properly initialized: a freshly started
    // server holds no sessions yet (the old assertion expected a non-empty
    // store at startup, which could never pass).
    assert!(state
        .session_manager
        .list_sessions_for_agent(&AgentId([0u8; 33]))
        .await
        .unwrap()
        .is_empty());
    assert!(!state.methods.is_empty());

    // Test method registration
    assert!(state.methods.contains("agent_getSession"));
    assert!(state.methods.contains("agent_getBalance"));
    assert!(state.methods.contains("agent_sponsorTransaction"));
}

#[tokio::test]
async fn test_session_lifecycle() {
    let config = ServerConfig::default();
    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    let agent_id = AgentId([0u8; 33]);
    let permissions = vec![
        strangecoin_api::session::SessionPermission::ReadAccount,
        strangecoin_api::session::SessionPermission::SubmitTransaction,
    ];

    let metadata = strangecoin_api::session::SessionMetadata::default();

    // Create a session
    let session = state
        .session_manager
        .create_session(agent_id, permissions, metadata)
        .await
        .unwrap();

    assert!(!session.session_id.is_empty());
    assert!(!session.is_expired());
    assert_eq!(session.permissions.len(), 2);

    // Get session info
    let retrieved_session = state
        .session_manager
        .get_session(&session.session_id)
        .await
        .unwrap();

    assert_eq!(retrieved_session.session_id, session.session_id);
    assert_eq!(retrieved_session.agent_id, session.agent_id);

    // Close session
    state
        .session_manager
        .close_session(&session.session_id)
        .await
        .unwrap();

    // Verify session is closed
    let result = state.session_manager.get_session(&session.session_id).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_method_execution() {
    let config = ServerConfig::default();
    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    let agent_id = AgentId([0u8; 33]);
    let permissions = vec![
        strangecoin_api::session::SessionPermission::ReadAccount,
        strangecoin_api::session::SessionPermission::SubmitTransaction,
    ];

    let metadata = strangecoin_api::session::SessionMetadata::default();

    // Create a session
    let session = state
        .session_manager
        .create_session(agent_id.clone(), permissions, metadata.clone())
        .await
        .unwrap();

    // Test agent_getSession method
    let result = state
        .methods
        .execute_method("agent_getSession", &session, Some(json!({})))
        .await
        .unwrap();

    assert!(result["session_id"].is_string());
    assert!(result["agent_id"].is_string());
    assert_eq!(result["session_id"], session.session_id);

    // Test agent_getBalance method
    let result = state
        .methods
        .execute_method("agent_getBalance", &session, Some(json!({})))
        .await
        .unwrap();

    assert!(result["balance"].is_string());
    assert!(result["pending"].is_string());
    assert_eq!(result["currency"], "STR");

    // Test permission error for restricted method
    let restricted_permissions = vec![strangecoin_api::session::SessionPermission::ReadAccount];

    let restricted_session = state
        .session_manager
        .create_session(agent_id, restricted_permissions, metadata)
        .await
        .unwrap();

    let result = state
        .methods
        .execute_method(
            "agent_sponsorTransaction",
            &restricted_session,
            Some(json!({
                "beneficiary": "0x1234567890abcdef",
                "amount": "1000000"
            })),
        )
        .await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(
            e.to_string(),
            "Permission denied: Access to 'agent_sponsorTransaction' denied"
        );
    }
}

#[tokio::test]
async fn test_batch_operations() {
    let config = ServerConfig::default();
    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    let agent_id = AgentId([0u8; 33]);
    // ReadAccount is required for agent_getBatchStatus (permission check
    // fires before execution); SubmitTransaction/ManageBatches gate the
    // signAndSubmitBatch call below.
    let permissions = vec![
        strangecoin_api::session::SessionPermission::ReadAccount,
        strangecoin_api::session::SessionPermission::SubmitTransaction,
        strangecoin_api::session::SessionPermission::ManageBatches,
    ];

    let metadata = strangecoin_api::session::SessionMetadata {
        max_batch_size: 10,
        ..Default::default()
    };

    // Create a session
    let session = state
        .session_manager
        .create_session(agent_id, permissions, metadata)
        .await
        .unwrap();

    // Test signAndSubmitBatch method
    let batch_transactions = vec![
        json!({
            "sender": "0x1234567890abcdef",
            "receiver": "0xabcdef1234567890",
            "amount": 1000000
        }),
        json!({
            "sender": "0x1234567890abcdef",
            "receiver": "0x9876543210fedcba",
            "amount": 500000
        }),
    ];

    let result = state
        .methods
        .execute_method(
            "agent_signAndSubmitBatch",
            &session,
            Some(json!({
                "transactions": batch_transactions
            })),
        )
        .await
        .unwrap();

    assert!(result["batch_id"].is_string());
    assert!(result["transactions"].is_array());
    assert_eq!(result["transactions"].as_array().unwrap().len(), 2);
    assert_eq!(result["total_count"], 2);
    assert_eq!(result["status"], "submitted");

    // Test batch status method
    let result = state
        .methods
        .execute_method(
            "agent_getBatchStatus",
            &session,
            Some(json!({
                "batch_id": result["batch_id"]
            })),
        )
        .await
        .unwrap();

    assert!(result["batch_id"].is_string());
    assert!(result["status"].is_string());
    assert!(result["processed_count"].is_number());
    assert!(result["total_count"].is_number());
}

#[tokio::test]
async fn test_sponsorship_operations() {
    let config = ServerConfig::default();
    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    let agent_id = AgentId([0u8; 33]);
    let permissions = vec![strangecoin_api::session::SessionPermission::SponsorTransactions];

    let metadata = strangecoin_api::session::SessionMetadata::default();

    // Create a session
    let session = state
        .session_manager
        .create_session(agent_id, permissions, metadata)
        .await
        .unwrap();

    // Test sponsorTransaction method
    let result = state
        .methods
        .execute_method(
            "agent_sponsorTransaction",
            &session,
            Some(json!({
                "beneficiary": "0x1234567890abcdef",
                "amount": "1000000",
                "max_fee": "10000"
            })),
        )
        .await
        .unwrap();

    assert!(result["sponsorship_id"].is_string());
    assert!(result["txid"].is_string());
    assert_eq!(result["beneficiary"], "0x1234567890abcdef");
    assert_eq!(result["amount"], "1000000");
    assert_eq!(result["fee"], "10000");
    assert_eq!(result["status"], "pending");

    // Test getSponsorships method
    let result = state
        .methods
        .execute_method("agent_getSponsorships", &session, Some(json!({})))
        .await
        .unwrap();

    assert!(result["sponsorships"].is_array());
    assert!(result["total_count"].is_number());
    assert!(result["total_amount"].is_string());
    assert!(result["total_fees"].is_string());

    // Test revokeSponsorship method
    let result = state
        .methods
        .execute_method(
            "agent_revokeSponsorship",
            &session,
            Some(json!({
                "sponsorship_id": result["sponsorships"][0]["sponsorship_id"]
            })),
        )
        .await
        .unwrap();

    assert_eq!(result["success"], true);
    assert!(result["sponsorship_id"].is_string());
    assert_eq!(result["message"], "Sponsorship revoked successfully");
}

#[tokio::test]
async fn test_activity_tracking() {
    let config = ServerConfig::default();
    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    let agent_id = AgentId([0u8; 33]);
    let permissions = vec![strangecoin_api::session::SessionPermission::ReadActivity];

    let metadata = strangecoin_api::session::SessionMetadata::default();

    // Create a session
    let session = state
        .session_manager
        .create_session(agent_id, permissions, metadata)
        .await
        .unwrap();

    // Test getActivity method
    let result = state
        .methods
        .execute_method(
            "agent_getActivity",
            &session,
            Some(json!({
                "limit": 10,
                "offset": 0
            })),
        )
        .await
        .unwrap();

    assert!(result["activities"].is_array());
    assert!(result["total_count"].is_number());
    assert_eq!(result["has_more"], false);

    let activities = result["activities"].as_array().unwrap();
    assert!(!activities.is_empty());

    let first_activity = &activities[0];
    assert!(first_activity["id"].is_string());
    assert!(first_activity["timestamp"].is_string());
    assert!(first_activity["type"].is_string());
    assert!(first_activity["details"].is_object());
}

#[tokio::test]
async fn test_rate_limiting() {
    // Rate limiting is enforced in the HTTP handler layer (handle_json_rpc)
    // via RateLimiter, NOT inside MethodRegistry::execute_method — so test
    // the limiter directly: first 3 requests pass, next 2 are rejected.
    let limiter = strangecoin_api::server::RateLimiter::new(3);
    let session_id = "agent_rate_limit_test";

    let mut allowed = 0;
    let mut denied = 0;

    for _ in 0..5 {
        if limiter.check_rate_limit(session_id).await {
            allowed += 1;
        } else {
            denied += 1;
        }
    }

    // Should have 3 successes and 2 rejections due to rate limiting
    assert_eq!(allowed, 3);
    assert_eq!(denied, 2);
}

#[tokio::test]
async fn test_session_expiration() {
    let config = ServerConfig {
        session_timeout: 1, // 1 second timeout
        ..Default::default()
    };

    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    let agent_id = AgentId([0u8; 33]);
    let permissions = vec![strangecoin_api::session::SessionPermission::ReadAccount];

    let metadata = strangecoin_api::session::SessionMetadata::default();

    // Create a session
    let session = state
        .session_manager
        .create_session(agent_id, permissions, metadata)
        .await
        .unwrap();

    // Wait for session to expire
    sleep(Duration::from_secs(2)).await;

    // Expiry is enforced by the store on lookup: execute_method receives an
    // already-fetched session snapshot and does NOT re-check expiry, so the
    // correct place to observe expiration is session_manager.get_session.
    let result = state.session_manager.get_session(&session.session_id).await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(
            e.to_string().starts_with("Session expired: agent_"),
            "unexpected error: {e}"
        );
    }
}

#[tokio::test]
async fn test_session_cleanup() {
    let config = ServerConfig::default();
    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    let agent_id = AgentId([0u8; 33]);
    let permissions = vec![strangecoin_api::session::SessionPermission::ReadAccount];

    let metadata = strangecoin_api::session::SessionMetadata::default();

    // Create multiple sessions
    let session1 = state
        .session_manager
        .create_session(agent_id.clone(), permissions.clone(), metadata.clone())
        .await
        .unwrap();

    let session2 = state
        .session_manager
        .create_session(agent_id.clone(), permissions.clone(), metadata.clone())
        .await
        .unwrap();

    let session3 = state
        .session_manager
        .create_session(agent_id, permissions, metadata)
        .await
        .unwrap();

    // Verify all sessions exist
    assert_eq!(
        state
            .session_manager
            .list_sessions_for_agent(&session1.agent_id)
            .await
            .unwrap()
            .len(),
        3
    );

    // Close all sessions for the agent
    let closed_count = state
        .session_manager
        .close_agent_sessions(&session1.agent_id)
        .await
        .unwrap();

    assert_eq!(closed_count, 3);

    // Verify all sessions are closed
    assert_eq!(
        state
            .session_manager
            .list_sessions_for_agent(&session1.agent_id)
            .await
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn test_error_handling() {
    let config = ServerConfig::default();
    let server = AgentApiServer::new(config).await.unwrap();
    let state = server.state().clone();

    let agent_id = AgentId([0u8; 33]);
    // SubmitTransaction is required for agent_signAndSubmitBatch — without it
    // the permission check fires before parameter validation and the
    // "Invalid parameters" assertion below would never see the real error.
    let permissions = vec![
        strangecoin_api::session::SessionPermission::ReadAccount,
        strangecoin_api::session::SessionPermission::SubmitTransaction,
    ];

    let metadata = strangecoin_api::session::SessionMetadata::default();

    // Create a session
    let session = state
        .session_manager
        .create_session(agent_id, permissions, metadata)
        .await
        .unwrap();

    // Test non-existent method
    let result = state
        .methods
        .execute_method("agent_nonExistentMethod", &session, Some(json!({})))
        .await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.to_string(), "Method not found: agent_nonExistentMethod");
    }

    // Test invalid parameters
    let result = state
        .methods
        .execute_method(
            "agent_signAndSubmitBatch",
            &session,
            Some(json!({
                "invalid_field": "invalid_value"
            })),
        )
        .await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("Invalid parameters"));
    }
}
