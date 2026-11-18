//! Strangecoin MCP (Model Context Protocol) server — ADR-0029 (AI-P02).
//!
//! Exposes chain state as MCP **tools** (7) and **resources** (4) over two
//! transports (spec [`docs/spec/mcp_server.md`](../../docs/spec/mcp_server.md)):
//!
//! - **stdio** ([`stdio`]): newline-delimited JSON-RPC on stdin/stdout, local
//!   only, no auth (process isolation, spec §4.1).
//! - **HTTP+SSE** ([`http_sse`], feature `http-sse`): `POST /mcp` +
//!   `GET /mcp/events`, every session authenticated with the Stage 2 Noise-XX
//!   handshake ([`auth`], ADR-0020 reuse) and payloads carried as
//!   ChaCha20-Poly1305 frames (spec §4.2).
//!
//! Three invariants shape the design (prompt-stage3.md §8.3 gates 3/4):
//!
//! 1. **Thin shell** — every handler is a pure function over the
//!    [`McpBackend`] trait; business logic stays in the monolith impl
//!    (root crate) / pure-core. No consensus rules are touched.
//! 2. **Output determinism** (ARCHITECT §19.5 #23) — tool output is a
//!    function of chain state only: no `SystemTime`, no RNG, no peer state.
//!    Canonical JSON goes through `serde_json::Value` (BTreeMap ⇒ sorted
//!    keys), so the same `get_block(h)` is byte-identical on every node.
//! 3. **Signed responses** (ARCHITECT §19.6 V-43, MCP tool poisoning) —
//!    every JSON-RPC response and every pushed event carries
//!    `signature = Ed25519(node_id_key, canonical_json(payload))` plus the
//!    signing `signer` pubkey; clients verify before trusting the payload.

pub mod auth;
pub mod resources;
pub mod signing;
pub mod stdio;
pub mod tools;

#[cfg(feature = "http-sse")]
pub mod http_sse;

// Re-exported so integration tests (root `tests/mcp_server.rs`) can build
// `axum::Request`/`StatusCode` values against the same pinned axum without
// adding a second root-level dependency edge.
pub use axum;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};
use strangecoin_core::types::{Block, BlockHeader, Transaction};
use strangecoin_core::vm::{Log, Receipt, TxStatus};
use strangecoin_net::NodeKeypair;

/// MCP spec version this server is pinned to (ADR-0029: upgrading requires
/// an ADR amendment).
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "strangecoin-mcp";
/// Cap for one JSON-RPC message on the stdio transport. stdio trusts the
/// parent process (spec §4.1), so this is a protocol-ordering guard rather
/// than a security boundary; HTTP bodies carry the same cap.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// Tool / backend failure. `InvalidParams` surfaces as JSON-RPC `-32602`;
/// everything else becomes a `tools/call` result with `isError: true`
/// (MCP treats tool execution failures as results, not protocol errors).
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("rejected: {0}")]
    Rejected(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("transport error: {0}")]
    Transport(String),
}

impl McpError {
    /// JSON-RPC 2.0 error code for protocol-level (non-tool) failures.
    pub fn jsonrpc_code(&self) -> i64 {
        match self {
            McpError::Parse(_) => -32700,
            McpError::InvalidRequest(_) => -32600,
            McpError::InvalidParams(_) => -32602,
            McpError::NotFound(_) => -32001,
            McpError::Rejected(_) => -32002,
            McpError::Unsupported(_) => -32003,
            McpError::Internal(_) | McpError::Transport(_) => -32603,
        }
    }
}

impl From<strangecoin_net::NoiseError> for McpError {
    fn from(err: strangecoin_net::NoiseError) -> Self {
        McpError::Transport(format!("noise: {err}"))
    }
}

/// Outcome of a read-only contract call (spec §2.5).
#[derive(Debug, Clone)]
pub struct CallOutcome {
    pub gas_used: u64,
    pub return_data: Vec<u8>,
    pub logs: Vec<Log>,
    pub status: TxStatus,
}

/// A transaction with its receipt when one is derivable (spec §3.3).
///
/// Receipts are derived data (invariant #18) and the node persists no
/// receipts CF (ADR-0024): a receipt is available only while the tx sits in
/// the current tip block (`tip_receipts`), otherwise `receipt` is `null` and
/// `height` still reports the confirmed location (S3-P09 index).
#[derive(Debug, Clone)]
pub struct TxRecord {
    pub tx: Transaction,
    pub receipt: Option<Receipt>,
    pub height: Option<u64>,
}

/// Contract info for `chain://contract/{address}` (spec §3.4).
#[derive(Debug, Clone)]
pub struct ContractInfo {
    pub code: Option<Vec<u8>>,
    pub storage_root: Option<[u8; 32]>,
    /// Always `None` on Stage 4: the node never persists ABIs (pre-existing
    /// gap — `WasmiExecutor::insert_abi` is never called by the monolith).
    pub abi_json: Option<String>,
}

/// Chain events surfaced through `subscribe_events` (spec §2.7).
///
/// `TxConfirmed`/`EventLog` are derived by the backend from the applied
/// block (the root `NodeEvent` bus carries `BlockApplied` only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event_type", rename_all = "PascalCase")]
pub enum McpEvent {
    BlockApplied {
        height: u64,
        hash: String,
    },
    TxConfirmed {
        txid: String,
        height: u64,
    },
    EventLog {
        height: u64,
        txid: String,
        address: String,
        topics: Vec<String>,
        data: String,
    },
}

/// Subscription filter for `subscribe_events` (spec §2.7). Pure predicate —
/// no clock, no state; identical inputs decide identically (#23).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventFilter {
    pub event_types: Option<Vec<String>>,
    pub addresses: Option<Vec<String>>,
    pub event_sigs: Option<Vec<String>>,
}

/// Event types accepted by the spec §2.7 `event_types` enum.
pub const EVENT_TYPES: [&str; 3] = ["BlockApplied", "TxConfirmed", "EventLog"];

impl EventFilter {
    /// Validate `event_types` against the spec enum; unknown names are
    /// invalid params (fail-closed rather than silently ignored).
    pub fn from_parts(
        event_types: Option<Vec<String>>,
        addresses: Option<Vec<String>>,
        event_sigs: Option<Vec<String>>,
    ) -> Result<Self, McpError> {
        if let Some(types) = &event_types {
            for t in types {
                if !EVENT_TYPES.contains(&t.as_str()) {
                    return Err(McpError::InvalidParams(format!(
                        "unknown event type `{t}` (expected one of {})",
                        EVENT_TYPES.join(", ")
                    )));
                }
            }
        }
        Ok(Self {
            event_types,
            addresses,
            event_sigs,
        })
    }

    pub fn matches(&self, event: &McpEvent) -> bool {
        if let Some(types) = &self.event_types {
            let name = match event {
                McpEvent::BlockApplied { .. } => "BlockApplied",
                McpEvent::TxConfirmed { .. } => "TxConfirmed",
                McpEvent::EventLog { .. } => "EventLog",
            };
            if !types.iter().any(|t| t == name) {
                return false;
            }
        }
        match event {
            McpEvent::EventLog {
                address, topics, ..
            } => {
                if let Some(addresses) = &self.addresses {
                    if !addresses.is_empty() && !addresses.iter().any(|a| a == address) {
                        return false;
                    }
                }
                if let Some(sigs) = &self.event_sigs {
                    if !sigs.is_empty()
                        && !sigs
                            .iter()
                            .any(|s| topics.iter().any(|t| hex::encode(t) == *s))
                    {
                        return false;
                    }
                }
                true
            }
            // Address/topic filters only make sense for EventLog entries; a
            // non-empty address/topic filter drops block/tx notifications.
            _ => {
                self.addresses.as_ref().is_none_or(|a| a.is_empty())
                    && self.event_sigs.as_ref().is_none_or(|s| s.is_empty())
            }
        }
    }
}

/// The node-side data plane every tool/resource is a pure function of.
///
/// Implemented for `BlockchainFacade` in the root crate behind the `mcp`
/// feature (same strangler split as `strangecoin_net::BlockchainView` —
/// gate 3: the impl lives outside pure-core, this crate never sees
/// monolith internals).
pub trait McpBackend: Send + Sync {
    /// Latest block header (`chain://tip`).
    fn tip_header(&self) -> Option<BlockHeader>;
    fn block_by_height(&self, height: u64) -> Option<Block>;
    /// `hash_hex` — 64 lowercase hex chars.
    fn block_by_hash(&self, hash_hex: &str) -> Option<Block>;
    /// Confirmed balance at the current tip, atomic units.
    fn get_balance(&self, address: &str) -> u64;
    fn get_nonce(&self, address: &str) -> u64;
    /// Confirmed balance adjusted by mempool spend/receive (spec §2.2
    /// `pending`); deterministic for a given (state, mempool) pair.
    fn pending_balance(&self, address: &str) -> u64;
    /// Validate + queue a signed transaction in the mempool. The txid is
    /// computed by the caller (`serialize::txid`) so rejections can still
    /// report it.
    fn submit_tx(&self, tx: Transaction) -> Result<[u8; 32], McpError>;
    /// Read-only VM execution (`staticcall` semantics, spec §2.5). `calldata`
    /// is passed verbatim; `gas_limit` bounds the frame.
    fn call_contract(
        &self,
        address: &str,
        calldata: Vec<u8>,
        gas_limit: u64,
    ) -> Result<CallOutcome, McpError>;
    /// Confirmed tx lookup (`chain://tx/{txid}`); `NotFound` when unknown.
    fn get_tx_with_receipt(&self, txid_hex: &str) -> Result<TxRecord, McpError>;
    /// Contract code + storage root; `NotFound` when the address is not a
    /// contract account.
    fn contract_info(&self, address: &str) -> Result<ContractInfo, McpError>;
    /// Fresh event subscription (`subscribe_events`).
    fn subscribe_events(&self) -> tokio::sync::mpsc::Receiver<McpEvent>;
}

/// Which transport a request arrived on — `subscribe_events` behaves
/// differently (spec §2.7: SSE stream on HTTP+SSE, notification push on
/// stdio).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Stdio,
    HttpSse,
}

/// Per-connection request context threaded through dispatch.
pub struct RequestCtx {
    pub transport: TransportKind,
    /// Set by `subscribe_events` on the stdio transport; the serve loop
    /// drains it after the ack line is written and spawns the push task.
    pending_subscription: std::sync::Mutex<Option<EventFilter>>,
}

impl RequestCtx {
    pub fn new(transport: TransportKind) -> Self {
        Self {
            transport,
            pending_subscription: std::sync::Mutex::new(None),
        }
    }

    pub fn take_subscription(&self) -> Option<EventFilter> {
        self.pending_subscription.lock().expect("ctx lock").take()
    }

    pub(crate) fn set_subscription(&self, filter: EventFilter) {
        *self.pending_subscription.lock().expect("ctx lock") = Some(filter);
    }
}

/// MCP server: JSON-RPC 2.0 dispatcher over an [`McpBackend`], signing every
/// response with the node's Ed25519 identity (V-43).
pub struct McpServer {
    backend: Arc<dyn McpBackend>,
    signer: NodeKeypair,
    shutdown: AtomicBool,
}

impl McpServer {
    pub fn new(backend: Arc<dyn McpBackend>, signer: NodeKeypair) -> Arc<Self> {
        Arc::new(Self {
            backend,
            signer,
            shutdown: AtomicBool::new(false),
        })
    }

    pub fn backend(&self) -> Arc<dyn McpBackend> {
        Arc::clone(&self.backend)
    }

    pub fn signer(&self) -> &NodeKeypair {
        &self.signer
    }

    /// Set by the `shutdown` request; serve loops check it after replying.
    pub fn shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Dispatch one JSON-RPC message. Returns `None` for notifications (and
    /// for messages that are neither requests nor notifications).
    pub fn handle(&self, request: Value, ctx: &RequestCtx) -> Option<Value> {
        let id = request.get("id").cloned();
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            // Not a request we can dispatch (a response object or garbage).
            return id.map(|id| {
                self.signed_error(id, &McpError::InvalidRequest("missing `method`".into()))
            });
        };
        let method = method.to_string();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        let id = match id {
            Some(id) => id,
            None => {
                self.handle_notification(&method, &params, ctx);
                return None;
            }
        };

        Some(match self.handle_request(&method, &params, ctx) {
            Ok(result) => self.signed_response(id, result),
            Err(err) => self.signed_error(id, &err),
        })
    }

    /// Signed JSON-RPC parse-error response (stdio line failed to decode —
    /// we never see the request id).
    pub fn parse_error_response(&self) -> Value {
        self.signed_error(
            Value::Null,
            &McpError::Parse("message is not valid JSON".into()),
        )
    }

    fn handle_notification(&self, method: &str, _params: &Value, _ctx: &RequestCtx) {
        // Lifecycle notifications (`notifications/initialized`,
        // `notifications/cancelled`) need no reply; anything else is ignored
        // per JSON-RPC (forward-compat, S2-P18 UnknownMessage precedent).
        tracing::debug!(target: "mcp", method, "notification received");
    }

    fn handle_request(
        &self,
        method: &str,
        params: &Value,
        ctx: &RequestCtx,
    ) -> Result<Value, McpError> {
        match method {
            "initialize" => Ok(self.initialize(params)),
            "ping" => Ok(json!({})),
            "shutdown" => {
                self.shutdown.store(true, Ordering::SeqCst);
                Ok(json!({}))
            }
            "tools/list" => Ok(json!({ "tools": tools::schemas() })),
            "tools/call" => self.tools_call(params, ctx),
            "resources/list" => Ok(json!({ "resources": resources::list() })),
            "resources/templates/list" => Ok(json!({
                "resourceTemplates": resources::templates()
            })),
            "resources/read" => self.resources_read(params),
            other => Err(McpError::Unsupported(format!("unknown method `{other}`"))),
        }
    }

    fn initialize(&self, params: &Value) -> Value {
        let requested = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(MCP_PROTOCOL_VERSION);
        // Pinned spec: answer with the version we implement; the client
        // decides whether to continue (MCP 2024-11-05 handshake rule).
        let version = if requested == MCP_PROTOCOL_VERSION {
            requested
        } else {
            MCP_PROTOCOL_VERSION
        };
        json!({
            "protocolVersion": version,
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false }
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Strangecoin chain state (ADR-0029). Every \
                response carries `signature` + `signer`: Ed25519 over the \
                canonical JSON of `result`/`error`."
        })
    }

    fn tools_call(&self, params: &Value, ctx: &RequestCtx) -> Result<Value, McpError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::InvalidParams("missing tool `name`".into()))?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        match tools::call(self, ctx, name, &args) {
            Ok(output) => Ok(call_tool_result(&output, false)),
            Err(McpError::InvalidParams(msg)) => Err(McpError::InvalidParams(msg)),
            // Tool execution failures are results with isError (MCP), so the
            // LLM sees the reason instead of a bare protocol error.
            Err(err) => Ok(call_tool_result(&json!(err.to_string()), true)),
        }
    }

    fn resources_read(&self, params: &Value) -> Result<Value, McpError> {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::InvalidParams("missing resource `uri`".into()))?;
        let (mime, value) = resources::read(self.backend.as_ref(), uri)?;
        let text = String::from_utf8(signing::canonical_json(&value))
            .map_err(|err| McpError::Internal(err.to_string()))?;
        Ok(json!({
            "contents": [{ "uri": uri, "mimeType": mime, "text": text }]
        }))
    }

    /// Envelope a successful result with its Ed25519 signature (V-43).
    pub fn signed_response(&self, id: Value, result: Value) -> Value {
        let signature = signing::sign(&self.signer, &result);
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
            "signature": signature,
            "signer": hex::encode(self.signer.public_key()),
        })
    }

    /// Envelope a JSON-RPC error with its Ed25519 signature (V-43: unsigned
    /// errors would let a MitM forge failures).
    pub fn signed_error(&self, id: Value, err: &McpError) -> Value {
        let error = json!({ "code": err.jsonrpc_code(), "message": err.to_string() });
        let signature = signing::sign(&self.signer, &error);
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": error,
            "signature": signature,
            "signer": hex::encode(self.signer.public_key()),
        })
    }
}

/// MCP `CallToolResult` envelope: the tool payload rides as canonical JSON
/// inside `content[0].text` (spec 2024-11-05 has no structured content;
/// output schemas in `docs/spec/mcp_server.md` describe that JSON).
fn call_tool_result(output: &Value, is_error: bool) -> Value {
    let text = if is_error {
        output
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| output.to_string())
    } else {
        String::from_utf8(signing::canonical_json(output)).unwrap_or_else(|_| output.to_string())
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

/// Canonical `Block` JSON (spec §2.1 outputSchema). Hashes/roots are hex
/// strings; `txs` use the canonical serde shape of each transaction.
pub(crate) fn block_json(block: &Block) -> Value {
    let txs: Vec<Value> = block
        .transactions
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    json!({
        "height": block.index,
        "hash": block.hash,
        "prev_hash": block.previous_hash,
        "timestamp": block.timestamp,
        "nonce": block.nonce,
        "target": block.target,
        "consensus_version": block.consensus_version,
        "tx_root": hex::encode(block.tx_root),
        "state_root": hex::encode(block.state_root),
        "receipts_root": hex::encode(block.receipts_root),
        "txs": txs,
    })
}

/// Canonical header JSON — `block_json` minus the transaction bodies
/// (`chain://tip`, spec §3.1).
pub(crate) fn header_json(header: &BlockHeader) -> Value {
    json!({
        "height": header.index,
        "hash": header.hash,
        "prev_hash": header.previous_hash,
        "timestamp": header.timestamp,
        "nonce": header.nonce,
        "target": header.target,
        "consensus_version": header.consensus_version,
        "tx_root": hex::encode(header.tx_root),
        "state_root": hex::encode(header.state_root),
        "receipts_root": hex::encode(header.receipts_root),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_filter_type_gating_is_pure() {
        let ev = McpEvent::BlockApplied {
            height: 7,
            hash: "aa".into(),
        };
        assert!(EventFilter::default().matches(&ev));
        let only_logs = EventFilter::from_parts(Some(vec!["EventLog".into()]), None, None).unwrap();
        assert!(!only_logs.matches(&ev));
        let only_blocks =
            EventFilter::from_parts(Some(vec!["BlockApplied".into()]), None, None).unwrap();
        assert!(only_blocks.matches(&ev));
    }

    #[test]
    fn event_filter_rejects_unknown_types() {
        let err = EventFilter::from_parts(Some(vec!["Nope".into()]), None, None).unwrap_err();
        assert!(matches!(err, McpError::InvalidParams(_)));
    }

    #[test]
    fn event_filter_address_filter_drops_non_log_events() {
        let ev = McpEvent::TxConfirmed {
            txid: "00".into(),
            height: 3,
        };
        let filtered = EventFilter::from_parts(None, Some(vec!["rsc1qqqq".into()]), None).unwrap();
        assert!(!filtered.matches(&ev));
    }

    #[test]
    fn block_json_is_sorted_key_canonical() {
        let block = Block {
            index: 1,
            timestamp: 42,
            transactions: vec![],
            previous_hash: "00".repeat(32),
            hash: "11".repeat(32),
            nonce: 0,
            target: "ff".repeat(32),
            consensus_version: 1,
            state_root: [1u8; 32],
            tx_root: [2u8; 32],
            receipts_root: [3u8; 32],
        };
        let a = signing::canonical_json(&block_json(&block));
        let b = signing::canonical_json(&block_json(&block));
        assert_eq!(a, b, "same block must serialize byte-identically");
        // serde_json::Value maps sort keys — `hash` before `height`.
        let text = String::from_utf8(a).unwrap();
        let hash_at = text.find("\"hash\"").unwrap();
        let height_at = text.find("\"height\"").unwrap();
        assert!(hash_at < height_at, "keys must be sorted: {text}");
    }
}
