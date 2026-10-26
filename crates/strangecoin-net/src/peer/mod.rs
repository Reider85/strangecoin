//! S2-P12 — peer management subsystem (ARCHITECT3 §3.6, ARCHITECT.md §11.6):
//! the store/manager/pool split.
//!
//! * [`store`] — persistent peer database + scoring/ban state (ADR-0022;
//!   the full weighted formula lands in `scoring.rs`, S2-P17).
//! * [`manager`] — connection orchestration, ban state machine, periodic
//!   tick, the outbound Noise+HELLO dial and the ADR-0023 `AddrSender` seam.
//! * [`pool`] — live [`pool::Connection`]s with the per-IP-range Sybil cap
//!   (threat V-17).

pub mod manager;
pub mod pool;
pub mod store;

pub use manager::{
    PeerManager, PeerManagerConfig, TickReport, DEFAULT_BAN_THRESHOLD, DEFAULT_MAX_CONNECTIONS,
    DEFAULT_MAX_PER_IP_RANGE, DEFAULT_PERSISTENT_BAN_DURATION, DEFAULT_PERSISTENT_THRESHOLD,
    DEFAULT_TEMP_BAN_DURATION, DEFAULT_WARN_THRESHOLD, TICK_INTERVAL,
};
pub use pool::{ip_range_key, Connection, ConnectionPool};
pub use store::{BanReason, BanRecord, PeerRecord, PeerStore, INITIAL_PEER_SCORE};
