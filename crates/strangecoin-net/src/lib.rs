//! Strangecoin network crate (Stage 2).
//!
//! Production-grade P2P subsystem: Noise Protocol Framework transport
//! (ADR-0020), Erlay reconciliation (ADR-0021), peer scoring (ADR-0022),
//! rate limiting (ADR-0023), gossip/compact blocks, peer discovery.
//!
//! Scaffolding created in S2-P05; `protocol`, `rate_limiter`, `sync` and
//! `sync_engine` were migrated from the monolith in S2-P06 (strangler
//! pattern); the Noise handshake state machine landed in S2-P07, the
//! encrypted transport framing in S2-P08, the Ed25519 `node_id` auth +
//! deterministic ephemeral keys in S2-P09 (`noise`), HELLO negotiation in
//! S2-P10, multi-source discovery in S2-P11, the peer store/manager/pool
//! split with the ban state machine in S2-P12 (`peer`), INV/GET_DATA
//! gossip in S2-P13 (`gossip`), BIP 152 compact blocks in S2-P14
//! (`gossip::compact_blocks`), Erlay reconciliation in S2-P15
//! (`gossip::erlay`), the token-bucket rate limiter in S2-P16
//! (`rate_limiter`), the ADR-0022 scoring formula in S2-P17
//! (`peer::scoring`) and the shadow replay tooling in S2-P20
//! (`shadow`). The remaining stub modules are populated by
//! the later Stage 2 prompts (see `analytics/prompt-stage2.md`).

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
pub use gossip::{
    announce_block, announce_tx, apply_block_txn, create_sketch, decode_difference,
    fallback_full_relay, fill_up, handle_get_block_txn, BlockTxn, CompactBlock, ErlayReconcile,
    FillOutcome, GetBlockTxn, GetData, GossipMessage, GossipState, Inv, InvKind, InventoryItem,
    NotFound, PeerReconciliation, ReconciliationConfig, ReconciliationState,
};
pub use noise::{
    verify_node_id, NodeId, NodeKeypair, NodeKeystore, NoiseError, NoiseHandshake, NoiseRole,
    NoiseTransport,
};
pub use peer::{
    run_connection, BanDecision, BanReason, BanRecord, Connection, ConnectionContext,
    ConnectionPool, PeerManager, PeerManagerConfig, PeerRecord, PeerStore, ScoringConfig,
    TickReport,
};
pub use rate_limiter::{
    parse_rate_spec, BucketSpec, MessageClass, MessageType, RateLimitConfig, RateLimitError,
    RateLimiter, TokenBucket,
};
pub use shadow::{
    compare, load_blocks_file, replay_blocks, BlockResult, BlockStatus, DivergenceReport,
    ReplayConfig, ReplayReport,
};
