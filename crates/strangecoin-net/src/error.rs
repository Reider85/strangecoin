use thiserror::Error;

/// Errors surfaced by the network subsystem.
///
/// Stage 2 taxonomy (S2-P05): handshake failures (Noise, S2-P07..S2-P09),
/// transport/framing failures, peer-management failures, protocol violations
/// (malformed or unexpected messages), and rate-limit violations
/// (ADR-0023, implemented in S2-P16).
#[derive(Debug, Error)]
pub enum NetError {
    /// Noise handshake failed (S2-P07..S2-P09).
    #[error("handshake error: {0}")]
    Handshake(String),

    /// Transport / framing failure (read, write, length-prefix, AEAD).
    #[error("transport error: {0}")]
    Transport(String),

    /// Peer management failure (connect, dial, peer store).
    #[error("peer error: {0}")]
    Peer(String),

    /// Protocol violation: malformed or unexpected message.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Rate-limit violation (ADR-0023).
    #[error("rate limit exceeded: {0}")]
    RateLimit(String),
}

impl From<std::io::Error> for NetError {
    fn from(err: std::io::Error) -> Self {
        NetError::Transport(err.to_string())
    }
}

impl From<crate::noise::NoiseError> for NetError {
    fn from(err: crate::noise::NoiseError) -> Self {
        NetError::Handshake(err.to_string())
    }
}

impl From<snow::Error> for NetError {
    fn from(err: snow::Error) -> Self {
        NetError::Handshake(err.to_string())
    }
}
