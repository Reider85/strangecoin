//! Noise-XX handshake helpers for the HTTP+SSE transport (ADR-0029 reuses
//! the Stage 2 handshake, ADR-0020 — no new crypto).
//!
//! `strangecoin_net` drives its handshake over `tokio::net::TcpStream`;
//! here the same three messages ride as HTTP request/response bodies
//! instead. The `NoiseHandshake` API is transport-agnostic (buffer in /
//! buffer out), so each POST advances one round:
//!
//! ```text
//! POST /mcp/handshake {"round":1}  →  msg2          (client sends msg1)
//! POST /mcp/handshake {"round":3}  →  session id    (client sends msg3)
//! ```
//!
//! Both sides then share a [`NoiseTransport`] whose `encrypt_frame` /
//! `decrypt_frame` (ChaCha20-Poly1305) wrap every `/mcp` payload.

use strangecoin_net::noise::transport::MAX_NOISE_MSG_LEN;
use strangecoin_net::{NodeId, NodeKeypair, NoiseError, NoiseHandshake, NoiseTransport};

use crate::McpError;

/// Server side of the XX handshake, held between the two POSTs.
pub struct ResponderHandshake {
    inner: NoiseHandshake,
    round1_done: bool,
}

/// Start a responder handshake for this node's identity.
pub fn responder(identity: &NodeKeypair) -> Result<ResponderHandshake, NoiseError> {
    let static_key = identity.noise_static_private_key();
    Ok(ResponderHandshake {
        inner: NoiseHandshake::responder(&static_key, identity, None)?,
        round1_done: false,
    })
}

impl ResponderHandshake {
    /// Round 1 → 2: consume the client's `msg1`, produce `msg2`.
    pub fn read_init(&mut self, msg1: &[u8]) -> Result<Vec<u8>, McpError> {
        if self.round1_done {
            return Err(McpError::InvalidRequest(
                "handshake round 1 already consumed".into(),
            ));
        }
        let mut scratch = vec![0u8; MAX_NOISE_MSG_LEN];
        self.inner
            .read_message(msg1, &mut scratch)
            .map_err(|err| McpError::Transport(format!("handshake msg1: {err}")))?;
        let written = self
            .inner
            .write_message(&[], &mut scratch)
            .map_err(|err| McpError::Transport(format!("handshake msg2: {err}")))?;
        self.round1_done = true;
        Ok(scratch[..written].to_vec())
    }

    /// Round 3: consume the client's `msg3` and promote to a transport.
    pub fn finish(mut self, msg3: &[u8]) -> Result<(NoiseTransport, NodeId), McpError> {
        if !self.round1_done {
            return Err(McpError::InvalidRequest(
                "handshake round 1 must complete before round 3".into(),
            ));
        }
        let mut scratch = vec![0u8; MAX_NOISE_MSG_LEN];
        self.inner
            .read_message(msg3, &mut scratch)
            .map_err(|err| McpError::Transport(format!("handshake msg3: {err}")))?;
        if !self.inner.is_finished() {
            return Err(McpError::Transport(
                "handshake not finished after msg3".into(),
            ));
        }
        self.inner
            .into_transport()
            .map_err(|err| McpError::Transport(format!("handshake promote: {err}")))
    }

    /// The running transcript — feeds the deterministic session id
    /// (blake3 of handshake hash + server counter; no RNG).
    pub fn handshake_hash(&self) -> Vec<u8> {
        self.inner.handshake_hash().to_vec()
    }
}

/// Client side (tests, SDK, remote agents): drives the same two POSTs.
pub struct InitiatorHandshake {
    inner: NoiseHandshake,
    stage: u8,
}

pub fn initiator(identity: &NodeKeypair) -> Result<InitiatorHandshake, NoiseError> {
    let static_key = identity.noise_static_private_key();
    Ok(InitiatorHandshake {
        inner: NoiseHandshake::initiator(&static_key, identity, None)?,
        stage: 0,
    })
}

impl InitiatorHandshake {
    /// Round 1: produce `msg1`.
    pub fn first_message(&mut self) -> Result<Vec<u8>, McpError> {
        if self.stage != 0 {
            return Err(McpError::Internal("first_message called twice".into()));
        }
        let mut scratch = vec![0u8; MAX_NOISE_MSG_LEN];
        let written = self
            .inner
            .write_message(&[], &mut scratch)
            .map_err(|err| McpError::Transport(format!("handshake msg1: {err}")))?;
        self.stage = 1;
        Ok(scratch[..written].to_vec())
    }

    /// Round 2 → 3: consume `msg2`, produce `msg3`.
    pub fn accept(&mut self, msg2: &[u8]) -> Result<Vec<u8>, McpError> {
        if self.stage != 1 {
            return Err(McpError::Internal("accept called out of order".into()));
        }
        let mut scratch = vec![0u8; MAX_NOISE_MSG_LEN];
        self.inner
            .read_message(msg2, &mut scratch)
            .map_err(|err| McpError::Transport(format!("handshake msg2: {err}")))?;
        let written = self
            .inner
            .write_message(&[], &mut scratch)
            .map_err(|err| McpError::Transport(format!("handshake msg3: {err}")))?;
        self.stage = 2;
        Ok(scratch[..written].to_vec())
    }

    /// Promote the finished handshake to a transport (after `msg3` sent).
    pub fn finish(self) -> Result<(NoiseTransport, NodeId), McpError> {
        if self.stage != 2 || !self.inner.is_finished() {
            return Err(McpError::Transport(
                "handshake not finished before promote".into(),
            ));
        }
        self.inner
            .into_transport()
            .map_err(|err| McpError::Transport(format!("handshake promote: {err}")))
    }
}

/// Deterministic session id: `blake3(handshake_hash || counter_be)` — the
/// transcript differs per handshake (fresh ephemerals, S2-P09) and the
/// counter separates exact replays of a captured transcript.
pub fn derive_id(handshake_hash: &[u8], counter: u64) -> String {
    let mut input = Vec::with_capacity(handshake_hash.len() + 8);
    input.extend_from_slice(handshake_hash);
    input.extend_from_slice(&counter.to_be_bytes());
    hex::encode(blake3::hash(&input).as_bytes())
}
