//! Strangecoin network crate (Stage 2).
//!
//! Production-grade P2P subsystem: Noise Protocol Framework transport
//! (ADR-0020), Erlay reconciliation (ADR-0021), peer scoring (ADR-0022),
//! rate limiting (ADR-0023), gossip/compact blocks, peer discovery.
//!
//! Scaffolding created in S2-P05; `protocol`, `rate_limiter`, `sync` and
//! `sync_engine` were migrated from the monolith in S2-P06 (strangler
//! pattern), the remaining modules are populated by the later Stage 2
//! prompts (see `analytics/prompt-stage2.md`).

pub mod blockchain_view;
pub mod discovery;
pub mod error;
pub mod gossip;
pub mod noise;
pub mod peer;
pub mod protocol;
pub mod rate_limiter;
pub mod shadow;
pub mod sync;
pub mod sync_engine;

pub use blockchain_view::BlockchainView;
pub use error::NetError;
