//! S2-P12 — `ConnectionPool`: active Noise connections + the per-IP-range
//! Sybil cap (ARCHITECT3 §3.6 / ARCHITECT.md §11.6: «ConnectionPool не знает
//! про скор — только держит открытые соединения»; threat V-17).
//!
//! One [`Connection`] owns one post-handshake TCP session: the
//! [`NoiseTransport`] (behind a `Mutex` — Stage 2 Pitfalls: `TransportState`
//! is not thread-safe, sharing needs a lock) plus the socket handle used for
//! egress. Reads are driven by the caller's loop (S2-P19 lands the async
//! read worker); everything this pool sends goes through
//! [`Connection::send_payload`], which seals the payload with
//! `encrypt_frame` and pushes the wire bytes out synchronously via
//! `try_write` — the sync path is what the S2-P11 `AddrSender` trait needs.
//!
//! **Determinism** (Stage 2 checklist #4): iteration for broadcast and for
//! limit accounting is ordered by `node_id`/range key, never by time or
//! insertion order.

use crate::error::NetError;
use crate::noise::{NodeId, NoiseTransport};
use crate::protocol::encrypt_frame;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::net::TcpStream;

/// One authenticated, post-handshake peer session.
pub struct Connection {
    node_id: NodeId,
    peer_addr: SocketAddr,
    capabilities: u32,
    transport: Mutex<NoiseTransport>,
    stream: TcpStream,
}

impl Connection {
    pub fn new(
        transport: NoiseTransport,
        stream: TcpStream,
        peer_addr: SocketAddr,
        node_id: NodeId,
        capabilities: u32,
    ) -> Self {
        Self {
            node_id,
            peer_addr,
            capabilities,
            transport: Mutex::new(transport),
            stream,
        }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    /// Negotiated capability bitfield (`local & remote`, S2-P10).
    pub fn capabilities(&self) -> u32 {
        self.capabilities
    }

    /// Encrypt one payload into a self-contained wire frame and push it to
    /// the socket.
    ///
    /// Synchronous on purpose: `AddrSender::send_addr` (S2-P11) is a sync
    /// trait, and `broadcast_message` drives the same path. Egress uses
    /// `try_write` in a loop; `WouldBlock` (socket send buffer full) surfaces
    /// as a transport error that broadcast logs and skips — full backpressure
    /// handling and a dedicated write task arrive with the async connection
    /// loop (S2-P19).
    pub fn send_payload(&self, payload: &[u8]) -> Result<(), NetError> {
        let mut frame = Vec::new();
        {
            let mut transport = lock_transport(&self.transport);
            encrypt_frame(&mut transport, payload, &mut frame)?;
        }
        let mut written = 0usize;
        while written < frame.len() {
            match self.stream.try_write(&frame[written..]) {
                Ok(0) => {
                    return Err(NetError::Transport(
                        "socket closed while writing frame".to_string(),
                    ))
                }
                Ok(n) => written += n,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    return Err(NetError::Transport(
                        "socket backpressure (try_write WouldBlock)".to_string(),
                    ))
                }
                Err(err) => return Err(NetError::Transport(err.to_string())),
            }
        }
        Ok(())
    }
}

fn lock_transport(mutex: &Mutex<NoiseTransport>) -> MutexGuard<'_, NoiseTransport> {
    // Poison → into_inner: the transport state stays consistent with the
    // frames already on the wire, deterministic recovery (FBP06 precedent).
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Active connections keyed by `node_id`, with the per-IP-range Sybil cap
/// (threat V-17: a flood of fake peers from one provider block exhausts the
/// connection table — cap them by range, not just by identity).
pub struct ConnectionPool {
    connections: HashMap<NodeId, Arc<Connection>>,
    max_per_ip_range: usize,
}

impl ConnectionPool {
    pub fn new(max_per_ip_range: usize) -> Self {
        Self {
            connections: HashMap::new(),
            max_per_ip_range,
        }
    }

    pub fn len(&self) -> usize {
        self.connections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Register a connection, enforcing the per-IP-range cap (V-17).
    ///
    /// Errors: duplicate `node_id` (already connected) and a full IP range.
    /// Total-count enforcement (`max_connections`) is the manager's job — the
    /// pool only knows about ranges (ARCHITECT.md §11.6).
    pub fn add_connection(&mut self, connection: Arc<Connection>) -> Result<(), NetError> {
        let node_id = connection.node_id();
        if self.connections.contains_key(&node_id) {
            return Err(NetError::Peer(format!(
                "peer {node_id} is already connected"
            )));
        }
        let ip = connection.peer_addr().ip();
        if self.count_by_ip_range(ip) >= self.max_per_ip_range {
            return Err(NetError::Peer(format!(
                "per-IP-range connection limit reached for {ip} (max {})",
                self.max_per_ip_range
            )));
        }
        self.connections.insert(node_id, connection);
        Ok(())
    }

    pub fn remove_connection(&mut self, node_id: &NodeId) -> Option<Arc<Connection>> {
        self.connections.remove(node_id)
    }

    pub fn get_connection(&self, node_id: &NodeId) -> Option<Arc<Connection>> {
        self.connections.get(node_id).cloned()
    }

    /// How many live connections share `ip`'s range (IPv4 /24, IPv6 /64).
    pub fn count_by_ip_range(&self, ip: IpAddr) -> usize {
        let key = ip_range_key(ip);
        self.connections
            .values()
            .filter(|conn| ip_range_key(conn.peer_addr().ip()) == key)
            .count()
    }

    /// Connected identities ordered by `node_id` bytes (determinism #4).
    pub fn node_ids_sorted(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.connections.keys().copied().collect();
        ids.sort();
        ids
    }
}

/// Range bucket for the Sybil cap: IPv4 → /24 (first three octets),
/// IPv6 → /64 (first eight bytes) — the standard provider-block granularity.
pub fn ip_range_key(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(v4) => v4.octets()[..3].to_vec(),
        IpAddr::V6(v6) => v6.octets()[..8].to_vec(),
    }
}
