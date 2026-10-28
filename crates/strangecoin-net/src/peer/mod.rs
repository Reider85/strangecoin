//! S2-P12 — peer management subsystem (ARCHITECT3 §3.6, ARCHITECT.md §11.6):
//! the store/manager/pool split.
//!
//! * [`store`] — persistent peer database + scoring/ban state (ADR-0022).
//! * [`scoring`] — the ADR-0022 weighted formula + ban band ladder
//!   (S2-P17: `ScoringConfig`, `compute_score`, `update_score`).
//! * [`manager`] — connection orchestration, ban state machine, periodic
//!   tick, the outbound Noise+HELLO dial, the inbound responder accept
//!   (S2-P19) and the ADR-0023 `AddrSender` seam.
//! * [`pool`] — live [`pool::Connection`]s with the per-IP-range Sybil cap
//!   (threat V-17).
//! * [`connection_loop`] — the async per-connection read worker (S2-P19):
//!   decrypt → rate-limit → dispatch (sync serve, gossip, compact blocks,
//!   Erlay).

pub mod connection_loop;
pub mod manager;
pub mod pool;
pub mod scoring;
pub mod store;

pub use connection_loop::{run_connection, ConnectionContext, IDLE_READ_TIMEOUT};
pub use manager::{
    PeerManager, PeerManagerConfig, TickReport, DEFAULT_MAX_CONNECTIONS, DEFAULT_MAX_PER_IP_RANGE,
    TICK_INTERVAL,
};
pub use pool::{ip_range_key, Connection, ConnectionPool};
pub use scoring::{
    BanDecision, ScoringConfig, DEFAULT_BAN_THRESHOLD, DEFAULT_PERSISTENT_BAN_DURATION,
    DEFAULT_PERSISTENT_THRESHOLD, DEFAULT_TEMP_BAN_DURATION, DEFAULT_WARN_THRESHOLD,
};
pub use store::{BanReason, BanRecord, PeerRecord, PeerStore, INITIAL_PEER_SCORE};
