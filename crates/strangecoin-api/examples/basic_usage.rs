//! Example usage of the Strangecoin Agent API

use std::sync::Arc;
use std::time::Duration;
use strangecoin_api::{
    auth::AgentAuth,
    session::{AgentSessionManager, InMemorySessionStore, SessionMetadata, SessionPermission},
    AgentApiServer, ServerConfig, ServerState,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Strangecoin Agent API Example...");

    // Create server configuration
    let config = ServerConfig {
        listen_addr: "127.0.0.1:9002".parse().unwrap(),
        session_timeout: 3600, // 1 hour
        max_sessions_per_agent: 10,
        enable_cors: true,
        enable_rate_limit: true,
        rate_limit_per_minute: 60,
    };

    // Create and start the server
    println!("Creating agent API server...");
    let server = AgentApiServer::new(config.clone()).await?;

    // Get a reference to the server state for demonstration
    let state = server.state().clone();

    println!("Server created successfully!");

    // Demonstrate session management
    demonstrate_session_management(&state).await?;

    // Demonstrate method execution
    demonstrate_method_execution(&state).await?;

    // Start the server (this will block until shutdown)
    println!("Starting server on {}", config.listen_addr);
    println!("Server endpoints:");
    println!("  - JSON-RPC: POST http://{}/rpc", config.listen_addr);
    println!("  - Health Check: GET http://{}/health", config.listen_addr);

    // Start the server
    server.serve().await?;

    Ok(())
}

async fn demonstrate_session_management(
    state: &strangecoin_api::ServerState,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Session Management Demo ===");

    // Create session store and manager
    let store = Arc::new(InMemorySessionStore::new());
    let auth = AgentAuth::new();
    let session_manager = Arc::new(AgentSessionManager::new(
        store,
        auth,
        Duration::from_secs(3600),
        10,
    ));

    // Create agent ID (in real usage, this would be derived from actual keys)
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);

    // Define session permissions
    let permissions = vec![
        SessionPermission::ReadAccount,
        SessionPermission::SubmitTransaction,
        SessionPermission::ManageBatches,
    ];

    // Create session metadata
    let metadata = SessionMetadata {
        ip_address: Some("192.168.1.100".to_string()),
        user_agent: Some("Example Agent/1.0".to_string()),
        max_batch_size: 50,
        rate_limit_per_minute: 30,
    };

    println!("Creating agent session...");
    let session = session_manager
        .create_session(agent_id, permissions, metadata)
        .await?;

    println!("✓ Session created: {}", session.session_id);
    println!("  - Agent ID: {}", hex::encode(&session.agent_id.0));
    println!("  - Permissions: {:?}", session.permissions);
    println!("  - Max batch size: {}", session.metadata.max_batch_size);
    println!(
        "  - Rate limit: {} req/min",
        session.metadata.rate_limit_per_minute
    );

    // Get session information
    println!("\nRetrieving session information...");
    let mut retrieved_session = session_manager.get_session(&session.session_id).await?;

    println!("✓ Session retrieved successfully");
    println!("  - Created at: {}", retrieved_session.created_at);
    println!("  - Expires at: {}", retrieved_session.expires_at);

    // Update session activity
    println!("\nUpdating session activity...");
    retrieved_session.update_activity();
    println!("✓ Session activity updated");

    // List all sessions for the agent
    println!("\nListing all sessions for agent...");
    let agent_sessions = session_manager
        .list_sessions_for_agent(&retrieved_session.agent_id)
        .await?;

    println!("✓ Found {} sessions for agent", agent_sessions.len());

    // Close session
    println!("\nClosing session...");
    session_manager.close_session(&session.session_id).await?;

    println!("✓ Session closed successfully");

    Ok(())
}

async fn demonstrate_method_execution(
    state: &strangecoin_api::ServerState,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Method Execution Demo ===");

    // Create a test session
    let agent_id = strangecoin_core::types::AgentId([0u8; 33]);
    let permissions = vec![
        SessionPermission::ReadAccount,
        SessionPermission::SubmitTransaction,
        SessionPermission::ManageBatches,
        SessionPermission::SponsorTransactions,
    ];

    let metadata = SessionMetadata::default();
    let session = state
        .session_manager
        .create_session(agent_id, permissions, metadata)
        .await?;

    println!("✓ Test session created: {}", session.session_id);

    // Demonstrate different method calls
    println!("\n1. Testing agent_getSession...");
    let session_info = state
        .methods
        .execute_method("agent_getSession", &session, Some(serde_json::json!({})))
        .await?;

    println!("✓ Session info retrieved");
    println!("  - Session ID: {}", session_info["session_id"]);
    println!("  - Agent ID: {}", session_info["agent_id"]);

    println!("\n2. Testing agent_getBalance...");
    let balance = state
        .methods
        .execute_method("agent_getBalance", &session, Some(serde_json::json!({})))
        .await?;

    println!("✓ Balance retrieved");
    println!("  - Balance: {}", balance["balance"]);
    println!("  - Pending: {}", balance["pending"]);
    println!("  - Currency: {}", balance["currency"]);

    println!("\n3. Testing agent_getNonce...");
    let nonce = state
        .methods
        .execute_method("agent_getNonce", &session, Some(serde_json::json!({})))
        .await?;

    println!("✓ Nonce retrieved");
    println!("  - Nonce: {}", nonce["nonce"]);

    println!("\n4. Testing agent_getActivity...");
    let activity = state
        .methods
        .execute_method(
            "agent_getActivity",
            &session,
            Some(serde_json::json!({
                "limit": 5,
                "offset": 0
            })),
        )
        .await?;

    println!("✓ Activity retrieved");
    println!("  - Total activities: {}", activity["total_count"]);
    println!("  - Has more: {}", activity["has_more"]);

    println!("\n5. Testing batch operations...");
    let batch_transactions = vec![
        serde_json::json!({
            "transfer": {
                "from": "0x1234567890abcdef",
                "to": "0xabcdef1234567890",
                "amount": "1000000"
            }
        }),
        serde_json::json!({
            "transfer": {
                "from": "0x1234567890abcdef",
                "to": "0x9876543210fedcba",
                "amount": "500000"
            }
        }),
    ];

    let batch_result = state
        .methods
        .execute_method(
            "agent_signAndSubmitBatch",
            &session,
            Some(serde_json::json!({
                "transactions": batch_transactions
            })),
        )
        .await?;

    println!("✓ Batch submitted");
    println!("  - Batch ID: {}", batch_result["batch_id"]);
    println!("  - Transaction count: {}", batch_result["total_count"]);
    println!("  - Status: {}", batch_result["status"]);

    println!("\n6. Testing sponsorship...");
    let sponsorship = state
        .methods
        .execute_method(
            "agent_sponsorTransaction",
            &session,
            Some(serde_json::json!({
                "beneficiary": "0x1234567890abcdef",
                "amount": "1000000",
                "max_fee": "10000"
            })),
        )
        .await?;

    println!("✓ Transaction sponsored");
    println!("  - Sponsorship ID: {}", sponsorship["sponsorship_id"]);
    println!("  - Beneficiary: {}", sponsorship["beneficiary"]);
    println!("  - Amount: {}", sponsorship["amount"]);
    println!("  - Status: {}", sponsorship["status"]);

    println!("\n7. Testing error handling...");
    let error_result = state
        .methods
        .execute_method(
            "agent_nonExistentMethod",
            &session,
            Some(serde_json::json!({})),
        )
        .await;

    match error_result {
        Err(e) => {
            println!("✓ Error handled correctly");
            println!("  - Error: {}", e);
        }
        Ok(_) => {
            println!("✗ Expected error but got success");
        }
    }

    // Clean up
    state
        .session_manager
        .close_session(&session.session_id)
        .await?;

    println!("\n✓ Test session closed");

    Ok(())
}

/// Example of how to make HTTP requests to the agent API
pub async fn example_http_requests() {
    println!("\n=== HTTP Request Examples ===");

    // Example using curl (would be done from external application)
    println!("Example curl commands:");
    println!("# Create session (not implemented in this example, would be done via auth endpoint)");
    println!("curl -X POST http://127.0.0.1:9002/rpc \\");
    println!("  -H 'Content-Type: application/json' \\");
    println!("  -H 'Authorization: Bearer <session_token>' \\");
    println!("  -d '{{");
    println!("    \"jsonrpc\": \"2.0\",");
    println!("    \"id\": 1,");
    println!("    \"method\": \"agent_getSession\",");
    println!("    \"params\": {{}}");
    println!("  }}'");

    println!("\n# Get balance");
    println!("curl -X POST http://127.0.0.1:9002/rpc \\");
    println!("  -H 'Content-Type: application/json' \\");
    println!("  -H 'Authorization: Bearer <session_token>' \\");
    println!("  -d '{{");
    println!("    \"jsonrpc\": \"2.0\",");
    println!("    \"id\": 2,");
    println!("    \"method\": \"agent_getBalance\",");
    println!("    \"params\": {{}}");
    println!("  }}'");

    println!("\n# Submit batch");
    println!("curl -X POST http://127.0.0.1:9002/rpc \\");
    println!("  -H 'Content-Type: application/json' \\");
    println!("  -H 'Authorization: Bearer <session_token>' \\");
    println!("  -d '{{");
    println!("    \"jsonrpc\": \"2.0\",");
    println!("    \"id\": 3,");
    println!("    \"method\": \"agent_signAndSubmitBatch\",");
    println!("    \"params\": {{");
    println!("      \"transactions\": [");
    println!("        {{");
    println!("          \"transfer\": {{");
    println!("            \"from\": \"0x1234567890abcdef\",");
    println!("            \"to\": \"0xabcdef1234567890\",");
    println!("            \"amount\": \"1000000\"");
    println!("          }}");
    println!("        }}");
    println!("      ]");
    println!("    }}");
    println!("  }}'");

    println!("\n# Health check");
    println!("curl http://127.0.0.1:9002/health");
}

/// Example of integration with existing Strangecoin components
pub async fn example_integration() {
    println!("\n=== Integration Example ===");

    // This shows how the agent API integrates with the existing Strangecoin system
    println!("Integration points:");
    println!("1. BlockchainFacade - used for all blockchain operations");
    println!("   - Balance queries");
    println!("   - Transaction submission");
    println!("   - State validation");
    println!("   - Witness verification");

    println!("\n2. ACP (Agent Communication Protocol) - used for agent-to-agent communication");
    println!("   - Session management");
    println!("   - Message signing");
    println!("   - Nonce validation");

    println!("\n3. Event System - used for real-time updates");
    println!("   - Block applied events");
    println!("   - Transaction confirmed events");
    println!("   - Agent activity events");

    println!("\n4. Configuration System - used for server settings");
    println!("   - Session timeouts");
    println!("   - Rate limits");
    println!("   - CORS settings");
    println!("   - Authentication settings");

    println!("\nThe agent API acts as a JSON-RPC facade over these existing components,");
    println!("providing a clean, agent-focused interface for external applications.");
}
