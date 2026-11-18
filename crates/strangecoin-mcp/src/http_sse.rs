//! HTTP+SSE transport (spec §4.2, feature `http-sse`).
//!
//! - `POST /mcp/handshake` — the Stage 2 Noise-XX handshake carried over
//!   HTTP POSTs ([`crate::auth`]); completes into an authenticated session.
//! - `POST /mcp` — one JSON-RPC message per request; body = base64
//!   ChaCha20-Poly1305 frame, session = `X-Mcp-Session` header. Unauthenticated
//!   requests get **401** (КГ: reject before any chain state is read).
//! - `GET /mcp/events` — SSE stream for `subscribe_events`; every `data:`
//!   line is an encrypted, signed `notifications/strangecoin/event`.
//!
//! **Session discipline (documented):** Noise enforces strictly ordered
//! nonces, so one session carries one in-flight request at a time (use a
//! single keep-alive connection, sequential calls). Create a *second*
//! session for the SSE stream — POST responses and SSE events would
//! otherwise interleave on the same send-nonce sequence.

use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde_json::{json, Value};
use strangecoin_net::protocol::{decrypt_frame, encrypt_frame};
use strangecoin_net::{NodeKeypair, NoiseTransport};

use crate::auth::{self, ResponderHandshake};
use crate::{
    signing, EventFilter, McpError, McpServer, RequestCtx, TransportKind, MAX_MESSAGE_BYTES,
};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// One authenticated HTTP session.
struct Session {
    /// Held for the whole request (decrypt → dispatch → encrypt) so send
    /// nonces stay in processing order; SSE holds it for the whole stream
    /// (one-purpose sessions — see module docs).
    transport: tokio::sync::Mutex<NoiseTransport>,
    kind: SessionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionKind {
    Rpc,
    Sse,
}

impl SessionKind {
    fn parse(raw: Option<&str>) -> Result<Self, McpError> {
        match raw.unwrap_or("rpc") {
            "rpc" => Ok(Self::Rpc),
            "sse" => Ok(Self::Sse),
            other => Err(McpError::InvalidParams(format!(
                "unknown session kind `{other}` (expected rpc|sse)"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Rpc => "rpc",
            Self::Sse => "sse",
        }
    }
}

pub struct HttpState {
    server: Arc<McpServer>,
    identity: NodeKeypair,
    /// In-flight handshakes between round 1 and round 3 POSTs.
    pending: tokio::sync::Mutex<HashMap<String, (ResponderHandshake, SessionKind)>>,
    sessions: tokio::sync::Mutex<HashMap<String, Arc<Session>>>,
    /// Monotonic transcript counter for deterministic session ids (no RNG).
    counter: AtomicU64,
}

impl HttpState {
    pub fn new(server: Arc<McpServer>, identity: NodeKeypair) -> Arc<Self> {
        Arc::new(Self {
            server,
            identity,
            pending: tokio::sync::Mutex::new(HashMap::new()),
            sessions: tokio::sync::Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
        })
    }

    pub fn server(&self) -> Arc<McpServer> {
        Arc::clone(&self.server)
    }

    pub fn identity(&self) -> &NodeKeypair {
        &self.identity
    }
}

/// Build the axum router for this server (bind with `axum::serve`).
pub fn router(server: Arc<McpServer>, identity: NodeKeypair) -> Router {
    router_with_state(HttpState::new(server, identity))
}

pub fn router_with_state(state: Arc<HttpState>) -> Router {
    Router::new()
        .route("/mcp/handshake", post(handshake))
        .route("/mcp", post(rpc))
        .route("/mcp/events", get(events))
        .layer(DefaultBodyLimit::max(MAX_MESSAGE_BYTES * 2))
        .with_state(state)
}

/// Bind `addr` and serve until the runtime shuts down (node wiring — keeps
/// `axum` out of the root crate's dependency graph).
pub async fn serve(
    server: Arc<McpServer>,
    identity: NodeKeypair,
    addr: std::net::SocketAddr,
) -> Result<(), McpError> {
    let router = router(server, identity);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| McpError::Transport(format!("bind {addr}: {err}")))?;
    tracing::info!(target: "mcp", %addr, "HTTP+SSE MCP listening");
    axum::serve(listener, router)
        .await
        .map_err(|err| McpError::Transport(format!("serve: {err}")))
}

fn b64_decode(field: &str, raw: &str) -> Result<Vec<u8>, McpError> {
    B64.decode(raw)
        .map_err(|err| McpError::InvalidParams(format!("`{field}` is not valid base64: {err}")))
}

fn b64_encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.into() })),
    )
        .into_response()
}

fn unauthorized(message: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": message }))).into_response()
}

/// `POST /mcp/handshake` — round 1 (with `msg1`) or round 3 (with `msg3`).
async fn handshake(State(state): State<Arc<HttpState>>, Json(req): Json<Value>) -> Response {
    match handle_handshake(&state, req).await {
        Ok(value) => Json(value).into_response(),
        Err(McpError::InvalidParams(msg)) | Err(McpError::InvalidRequest(msg)) => bad_request(msg),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_handshake(state: &HttpState, req: Value) -> Result<Value, McpError> {
    let round = req
        .get("round")
        .and_then(Value::as_u64)
        .ok_or_else(|| McpError::InvalidParams("missing `round` (1|3)".into()))?;
    let message = b64_decode(
        "message",
        req.get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::InvalidParams("missing base64 `message`".into()))?,
    )?;

    match round {
        1 => {
            let kind = SessionKind::parse(req.get("kind").and_then(Value::as_str))?;
            let mut hs = auth::responder(state.identity())?;
            let msg2 = hs.read_init(&message)?;
            let handshake_id = hex::encode(blake3::hash(&message).as_bytes());
            state
                .pending
                .lock()
                .await
                .insert(handshake_id.clone(), (hs, kind));
            Ok(json!({
                "handshake_id": handshake_id,
                "message": b64_encode(&msg2),
            }))
        }
        3 => {
            let handshake_id = req
                .get("handshake_id")
                .and_then(Value::as_str)
                .ok_or_else(|| McpError::InvalidParams("missing `handshake_id`".into()))?
                .to_string();
            let (hs, kind) = state
                .pending
                .lock()
                .await
                .remove(&handshake_id)
                .ok_or_else(|| McpError::NotFound(format!("unknown handshake `{handshake_id}`")))?;
            let transcript = hs.handshake_hash();
            let (transport, _peer_node_id) = hs.finish(&message)?;
            let counter = state.counter.fetch_add(1, Ordering::Relaxed);
            let session_id = auth::derive_id(&transcript, counter);
            state.sessions.lock().await.insert(
                session_id.clone(),
                Arc::new(Session {
                    transport: tokio::sync::Mutex::new(transport),
                    kind,
                }),
            );
            Ok(json!({
                "session": session_id,
                "kind": kind.as_str(),
                // The server's own identity: `node_id == blake3(signer)` is
                // the Stage 2 binding clients check response signatures by.
                "node_id": hex::encode(state.identity().node_id().as_bytes()),
                "signer": hex::encode(state.identity().public_key()),
            }))
        }
        other => Err(McpError::InvalidParams(format!(
            "unknown handshake `round` {other} (expected 1|3)"
        ))),
    }
}

/// `POST /mcp` — one Noise-framed JSON-RPC message per request.
async fn rpc(State(state): State<Arc<HttpState>>, headers: HeaderMap, body: Bytes) -> Response {
    let session_id = match headers
        .get("X-Mcp-Session")
        .and_then(|value| value.to_str().ok())
    {
        Some(id) => id.to_string(),
        None => {
            return unauthorized(
                "authentication required: Noise-XX session via POST /mcp/handshake",
            )
        }
    };

    let session = {
        let sessions = state.sessions.lock().await;
        match sessions.get(&session_id) {
            Some(session) if session.kind == SessionKind::Rpc => Arc::clone(session),
            Some(_) => return bad_request("session kind mismatch: this session is SSE-only"),
            None => return unauthorized("unknown or expired session"),
        }
    };

    let raw = match std::str::from_utf8(&body)
        .map_err(|_| McpError::InvalidParams("body must be base64 text".into()))
        .and_then(|text| b64_decode("body", text))
    {
        Ok(raw) => raw,
        Err(err) => return bad_request(err.to_string()),
    };

    // One lock scope = decrypt → dispatch → encrypt: send/recv nonces move
    // exactly once per completed request, in processing order (the
    // per-session ordering discipline from the module docs).
    let mut transport = session.transport.lock().await;
    let plaintext = match decrypt_frame(&mut transport, &raw) {
        Ok(plaintext) => plaintext,
        Err(err) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": format!("decrypt failed: {err}") })),
            )
                .into_response()
        }
    };
    let request: Value = match serde_json::from_slice(&plaintext) {
        Ok(value) => value,
        Err(err) => return bad_request(format!("payload is not valid JSON: {err}")),
    };

    let ctx = RequestCtx::new(TransportKind::HttpSse);
    let Some(response) = state.server.handle(request, &ctx) else {
        // Notification — nothing to frame back.
        return StatusCode::ACCEPTED.into_response();
    };
    let response_bytes = match serde_json::to_vec(&response) {
        Ok(bytes) => bytes,
        Err(err) => return bad_request(format!("response serialization failed: {err}")),
    };
    let mut frame = Vec::new();
    if let Err(err) = encrypt_frame(&mut transport, &response_bytes, &mut frame) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("encrypt failed: {err}") })),
        )
            .into_response();
    }
    drop(transport);

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain")],
        b64_encode(&frame),
    )
        .into_response()
}

/// `GET /mcp/events` — SSE stream of encrypted, signed event notifications.
async fn events(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let session_id = match query.get("session") {
        Some(id) => id.clone(),
        None => {
            return unauthorized(
                "authentication required: Noise-XX session via POST /mcp/handshake (kind=sse)",
            )
        }
    };

    let session = {
        let sessions = state.sessions.lock().await;
        match sessions.get(&session_id) {
            Some(session) if session.kind == SessionKind::Sse => Arc::clone(session),
            Some(_) => return bad_request("session kind mismatch: this session is RPC-only"),
            None => return unauthorized("unknown or expired session"),
        }
    };

    let filter = match filter_from_query(&query) {
        Ok(filter) => filter,
        Err(err) => return bad_request(err.to_string()),
    };

    let mut events_rx = state.server.backend().subscribe_events();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(16);
    let signer = state.identity().clone();

    tokio::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            if !filter.matches(&event) {
                continue;
            }
            let Ok(event_value) = serde_json::to_value(&event) else {
                continue;
            };
            let signature = signing::sign(&signer, &event_value);
            let notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/strangecoin/event",
                "params": {
                    "event": event_value,
                    "signature": signature,
                    "signer": hex::encode(signer.public_key()),
                }
            });
            let Ok(bytes) = serde_json::to_vec(&notification) else {
                continue;
            };
            // Seal inside the task (async lock), never inside Stream::poll.
            let sealed = {
                let mut transport = session.transport.lock().await;
                let mut frame = Vec::new();
                match encrypt_frame(&mut transport, &bytes, &mut frame) {
                    Ok(()) => b64_encode(&frame),
                    Err(_) => break, // Session unusable — end the stream.
                }
            };
            if tx.send(Ok(Event::default().data(sealed))).await.is_err() {
                break; // Client disconnected.
            }
        }
    });

    Sse::new(EventStream { rx }).into_response()
}

fn filter_from_query(query: &HashMap<String, String>) -> Result<EventFilter, McpError> {
    let list = |key: &str| -> Option<Vec<String>> {
        query.get(key).and_then(|raw| {
            let items: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            (!items.is_empty()).then_some(items)
        })
    };
    EventFilter::from_parts(list("types"), list("addresses"), list("event_sigs"))
}

/// `futures_core::Stream` adapter over the event channel (avoids pulling
/// `tokio-stream` just for one wrapper).
struct EventStream {
    rx: tokio::sync::mpsc::Receiver<Result<Event, Infallible>>,
}

impl futures_core::Stream for EventStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().rx.poll_recv(cx)
    }
}
