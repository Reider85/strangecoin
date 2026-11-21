//! Strangecoin API - Agent-Native JSON-RPC Surface
//!
//! This crate provides a JSON-RPC 2.0 interface for agent operations,
//! implementing the agent_* methods specified in AI-P04.
//!
//! # Architecture
//!
//! - **JSON-RPC Server**: HTTP server using axum framework
//! - **Session Management**: Agent session lifecycle and authentication
//! - **Agent Methods**: Implementation of agent_* JSON-RPC methods
//! - **Integration**: Seamless integration with existing BlockchainFacade
//!
//! # Features
//!
//! - Session-based authentication with nonce validation
//! - Agent transaction batching and sponsorship
//! - Activity tracking and monitoring
//! - Permission-based access control
//!
//! # Usage
//!
//! ```no_run
//! use strangecoin_api::{ServerConfig, AgentApiServer};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = ServerConfig::default();
//!     let server = AgentApiServer::new(config).await.unwrap();
//!     server.serve().await.unwrap();
//! }
//! ```

pub mod auth;
pub mod error;
pub mod methods;
pub mod server;
pub mod session;

pub use auth::{AgentAuth, SessionToken};
pub use error::{AgentError, AgentResult};
pub use server::{AgentApiServer, ServerConfig, ServerState};
pub use session::{AgentSession, AgentSessionManager};

// Re-export commonly used types
pub use strangecoin_core::{types::*, Transaction};
pub use uuid::Uuid;
