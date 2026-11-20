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
//! ```rust
//! use strangecoin_api::{ServerConfig, AgentApiServer};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = ServerConfig::default();
//!     let server = AgentApiServer::new(config).await.unwrap();
//!     server.serve().await.unwrap();
//! }
//! ```

pub mod server;
pub mod session;
pub mod auth;
pub mod methods;
pub mod error;

pub use server::{AgentApiServer, ServerConfig, ServerState};
pub use session::{AgentSession, AgentSessionManager};
pub use auth::{AgentAuth, SessionToken};
pub use error::{AgentError, AgentResult};

// Re-export commonly used types
pub use strangecoin_core::{types::*, Transaction};
pub use uuid::Uuid;