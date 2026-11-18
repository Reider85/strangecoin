//! AI-P02 (ADR-0029) — MCP server integration suite.
//!
//! Covers the prompt КГ tests (`mcp_stdio_get_block_round_trip`,
//! `mcp_http_sse_auth`, `mcp_response_signature`,
//! `mcp_cli_reference_compat`) plus the spec §9 extras
//! (`mcp_determinism_3_nodes`, `mcp_subscribe_events_sse`,
//! `mcp_resource_chain_tip`) and an always-on scripted `mcp-cli` lifecycle
//! round-trip that runs without network.
//!
//! Chain fixtures reuse the `tests/state_root.rs` adopt pattern: one source
//! node builds the chain (grant + crafted transfer), each "remote node"
//! adopts the identical candidate — same bytes, same state, three servers.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use strangecoin::events::NodeEvent;
use strangecoin::BlockchainFacade;
use strangecoin_mcp::auth;
use strangecoin_mcp::axum;
use strangecoin_mcp::{signing, McpBackend, McpServer, RequestCtx, TransportKind};
use strangecoin_net::protocol::{decrypt_frame, encrypt_frame};
use strangecoin_net::NodeKeypair;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;

/// Fixed signing seed: all three cross-node servers share one identity, so
/// the whole response envelope (including the Ed25519 signature) is
/// byte-identical (RFC 8032 signatures are deterministic per key).
fn server_identity() -> NodeKeypair {
    NodeKeypair::from_seed([42u8; 32])
}

/// Fixed client seed for HTTP handshake tests.
fn client_identity() -> NodeKeypair {
    NodeKeypair::from_seed([43u8; 32])
}

fn server_for(bc: &BlockchainFacade) -> Arc<McpServer> {
    let backend: Arc<dyn McpBackend> = Arc::new(bc.clone());
    McpServer::new(backend, server_identity())
}

/// Build `[genesis, grant(alice 10000), transfer(alice→bob 500)]` on one
/// facade (state_root.rs `committed_chain` pattern).
fn seed_chain(tag: &str) -> (TestDir, BlockchainFacade, String, String) {
    let dir = TestDir::new(tag);
    let bc = create_test_blockchain(dir.path());
    let keypairs = generate_keypairs(2);
    let alice = keypairs[0].0.clone();
    let bob = keypairs[1].0.clone();
    assert!(
        bc.grant_initial_balance_to_first_wallet(&alice)
            .expect("grant must apply"),
        "grant block must be created"
    );

    let prefix = bc.chain_snapshot();
    let mut transfer = strangecoin::Transaction::transfer(
        &alice,
        &bob,
        500,
        1,
        strangecoin::consensus::CHAIN_ID_REGTEST,
    );
    sign_transaction(&mut transfer, &keypairs[0].1);
    let child = craft_child(&prefix, vec![coinbase_tx("miner", 0), transfer]);
    bc.add_block(child).expect("crafted child must apply");
    assert_eq!(bc.chain_len(), 3, "genesis + grant + transfer");
    (dir, bc, alice, bob)
}

/// N fresh facades, each adopting `chain` verbatim.
fn adopt_nodes(chain: &[strangecoin::Block], tag: &str) -> Vec<(TestDir, BlockchainFacade)> {
    (0..3)
        .map(|i| {
            let dir = TestDir::new(&format!("{tag}_{i}"));
            let bc = create_test_blockchain(dir.path());
            let adopted = bc
                .adopt_candidate(chain.to_vec(), None, Vec::new(), 0)
                .expect("identical chain must adopt");
            assert!(adopted, "node {i} must adopt the seeded chain");
            (dir, bc)
        })
        .collect()
}

// ------------------------------------------------------------- stdio client

struct Stdio {
    writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    reader: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    task: tokio::task::JoinHandle<Result<(), strangecoin_mcp::McpError>>,
}

impl Stdio {
    fn spawn(server: Arc<McpServer>) -> Self {
        let (server_end, client_end) = tokio::io::duplex(64 * 1024);
        let (srv_read, srv_write) = tokio::io::split(server_end);
        let task = tokio::spawn(strangecoin_mcp::stdio::serve_stdio(
            server, srv_read, srv_write,
        ));
        let (client_read, client_write) = tokio::io::split(client_end);
        Self {
            writer: client_write,
            reader: BufReader::new(client_read).lines(),
            task,
        }
    }

    async fn send(&mut self, request: Value) {
        let mut line = serde_json::to_vec(&request).expect("request serializes");
        line.push(b'\n');
        self.writer.write_all(&line).await.expect("write request");
        self.writer.flush().await.expect("flush request");
    }

    /// Send a request and return the raw response line (kept as text so the
    /// 3-node byte comparison sees the exact wire bytes).
    async fn call(&mut self, request: Value) -> String {
        self.send(request).await;
        loop {
            let line = self
                .reader
                .next_line()
                .await
                .expect("stdio read")
                .expect("server must not close on a live request");
            if !line.trim().is_empty() {
                return line;
            }
        }
    }

    async fn shutdown(self) {
        // Both halves must go: dropping only the writer leaves the reader
        // half of the same duplex alive and the server never sees EOF.
        let Stdio {
            writer,
            reader,
            task,
        } = self;
        drop(writer);
        drop(reader);
        let result = task.await.expect("serve task panics");
        assert!(result.is_ok(), "serve_stdio exits cleanly: {result:?}");
    }
}

// ------------------------------------------------------------- КГ #1: stdio

#[tokio::test]
async fn mcp_stdio_get_block_round_trip() {
    let (_source_dir, source, _alice, _bob) = seed_chain("mcp_stdio_rt");
    let chain = source.chain_snapshot();
    let nodes = adopt_nodes(&chain, "mcp_stdio_rt");

    let mut clients: Vec<Stdio> = nodes
        .iter()
        .map(|(_, bc)| Stdio::spawn(server_for(bc)))
        .collect();

    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                    "clientInfo": {"name": "cross-node-test", "version": "0"}}
    });
    let mut init_lines = Vec::new();
    for client in clients.iter_mut() {
        init_lines.push(client.call(initialize.clone()).await);
    }
    assert_eq!(init_lines[0], init_lines[1], "initialize identical");
    assert_eq!(init_lines[1], init_lines[2], "initialize identical");

    for client in clients.iter_mut() {
        client
            .send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
    }

    let get_block = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "get_block", "arguments": {"height": 1}}
    });
    let mut responses = Vec::new();
    for client in clients.iter_mut() {
        responses.push(client.call(get_block.clone()).await);
    }

    // КГ: byte-identical across N=3 nodes (#23).
    assert_eq!(responses[0], responses[1], "get_block(1) byte-identical");
    assert_eq!(responses[1], responses[2], "get_block(1) byte-identical");

    let parsed: Value = serde_json::from_str(&responses[0]).expect("response is JSON");
    signing::verify_envelope(&parsed).expect("response signature verifies");
    let text = parsed["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text content");
    let block: Value = serde_json::from_str(text).expect("tool payload is JSON");
    assert_eq!(block["height"], 1, "grant block at height 1");
    assert_eq!(
        block["hash"].as_str().map(str::len),
        Some(64),
        "canonical hex hash"
    );

    for client in clients {
        client.shutdown().await;
    }
}

// ------------------------------------------------------------- КГ #2: auth

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64d(text: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .expect("base64 body")
}

fn post_plain(
    uri: &str,
    body: &str,
    session: Option<&str>,
) -> axum::http::Request<axum::body::Body> {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "text/plain");
    if let Some(session) = session {
        builder = builder.header("X-Mcp-Session", session);
    }
    builder
        .body(axum::body::Body::from(body.to_string()))
        .expect("request")
}

/// JSON bodies (the handshake route extracts `Json<Value>` — axum rejects
/// any other content type with 415).
fn post_json(uri: &str, value: &Value) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(value.to_string()))
        .expect("request")
}

fn get_uri(uri: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .body(axum::body::Body::empty())
        .expect("request")
}

/// Client side of the two-POST Noise-XX handshake (spec §4.2).
async fn http_handshake(
    router: &axum::Router,
    kind: &str,
) -> (String, strangecoin_net::NoiseTransport) {
    let mut initiator = auth::initiator(&client_identity()).expect("initiator builds");
    let msg1 = initiator.first_message().expect("msg1");

    let req = json!({"round": 1, "kind": kind, "message": b64(&msg1)});
    let resp = router
        .clone()
        .oneshot(post_json("/mcp/handshake", &req))
        .await
        .expect("handshake round 1 reaches the server");
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::OK,
        "round 1 accepted"
    );
    let body = to_bytes(resp).await;
    let round2: Value = serde_json::from_slice(&body).expect("round 1 response JSON");
    let handshake_id = round2["handshake_id"]
        .as_str()
        .expect("handshake_id")
        .to_string();
    let msg3 = initiator
        .accept(&b64d(round2["message"].as_str().expect("msg2")))
        .expect("msg3 builds");

    let req = json!({"round": 3, "handshake_id": handshake_id, "message": b64(&msg3)});
    let resp = router
        .clone()
        .oneshot(post_json("/mcp/handshake", &req))
        .await
        .expect("handshake round 3 reaches the server");
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::OK,
        "round 3 accepted"
    );
    let body = to_bytes(resp).await;
    let done: Value = serde_json::from_slice(&body).expect("round 3 response JSON");
    let session = done["session"].as_str().expect("session id").to_string();
    // V-43 binding: advertised signer is the server identity, node_id =
    // blake3(signer) matches the Noise-authenticated peer id.
    let signer = done["signer"].as_str().expect("signer");
    assert_eq!(signer, hex::encode(server_identity().public_key()));
    let (transport, peer) = initiator.finish().expect("handshake promotes");
    assert_eq!(
        hex::encode(peer.as_bytes()),
        done["node_id"].as_str().expect("node_id")
    );
    (session, transport)
}

async fn to_bytes(resp: axum::http::Response<axum::body::Body>) -> Vec<u8> {
    resp.into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn mcp_http_sse_auth() {
    let (_dir, bc, alice, _bob) = seed_chain("mcp_http_auth");
    let router = strangecoin_mcp::http_sse::router(server_for(&bc), server_identity());
    let expected_nonce = bc.get_nonce(&alice);

    // Unauthenticated POST /mcp → 401 (reject before touching chain state).
    let resp = router
        .clone()
        .oneshot(post_plain("/mcp", "AAAA", None))
        .await
        .expect("request reaches the server");
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::UNAUTHORIZED,
        "no session → 401"
    );

    // Unauthenticated SSE → 401.
    let resp = router
        .clone()
        .oneshot(get_uri("/mcp/events"))
        .await
        .expect("request");
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::UNAUTHORIZED,
        "no session → 401"
    );

    // Unknown session → 401.
    let resp = router
        .clone()
        .oneshot(post_plain("/mcp", "AAAA", Some("deadbeef")))
        .await
        .expect("request");
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::UNAUTHORIZED,
        "unknown session"
    );

    // Noise-XX handshake → authorized, encrypted, signed round-trip.
    let (session, mut transport) = http_handshake(&router, "rpc").await;
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "get_nonce", "arguments": {"address": alice}}
    });
    let mut frame = Vec::new();
    encrypt_frame(
        &mut transport,
        &serde_json::to_vec(&request).unwrap(),
        &mut frame,
    )
    .expect("seal request");
    let resp = router
        .clone()
        .oneshot(post_plain("/mcp", &b64(&frame), Some(&session)))
        .await
        .expect("request");
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::OK,
        "authenticated call passes"
    );
    let body = to_bytes(resp).await;
    let plaintext = decrypt_frame(
        &mut transport,
        &b64d(std::str::from_utf8(&body).expect("b64 body")),
    )
    .expect("response decrypts");
    let response: Value = serde_json::from_slice(&plaintext).expect("response JSON");
    signing::verify_envelope(&response).expect("signed response verifies");
    let payload: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text content"),
    )
    .expect("tool payload JSON");
    assert_eq!(
        payload["nonce"], expected_nonce,
        "nonce read through the same facade"
    );

    // Kind mismatch: an SSE session must not serve POST /mcp.
    let (sse_session, _sse_transport) = http_handshake(&router, "sse").await;
    let resp = router
        .clone()
        .oneshot(post_plain("/mcp", "AAAA", Some(&sse_session)))
        .await
        .expect("request");
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::BAD_REQUEST,
        "kind mismatch → 400"
    );
}

// ------------------------------------------------- spec §9: SSE subscription

#[tokio::test]
async fn mcp_subscribe_events_sse() {
    let (_dir, bc, _alice, _bob) = seed_chain("mcp_sse");
    let tip = bc.tip().expect("seeded chain has a tip");
    let router = strangecoin_mcp::http_sse::router(server_for(&bc), server_identity());
    let (session, mut transport) = http_handshake(&router, "sse").await;

    let resp = router
        .clone()
        .oneshot(get_uri(&format!("/mcp/events?session={session}")))
        .await
        .expect("events request");
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::OK,
        "authenticated SSE opens"
    );
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "SSE content type, got {content_type}"
    );

    // The subscription registered inside the handler (EventBus::subscribe is
    // synchronous); the bridge pumps within one 50 ms poll interval.
    bc.event_bus().publish(NodeEvent::BlockApplied {
        height: tip.index,
        hash: tip.hash.clone(),
    });

    let mut body = resp.into_body();
    let mut buffer = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let data = loop {
        let frame = tokio::time::timeout_at(deadline, body.frame())
            .await
            .expect("SSE data frame within 5s")
            .expect("body stays open")
            .expect("frame decodes");
        let chunk = frame.into_data().expect("data frame");
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        if let Some(start) = buffer.find("data: ") {
            let rest = &buffer[start + "data: ".len()..];
            if let Some(end) = rest.find('\n') {
                break rest[..end].trim_end_matches('\r').to_string();
            }
        }
    };

    let plaintext = decrypt_frame(&mut transport, &b64d(&data)).expect("event decrypts");
    let notification: Value = serde_json::from_slice(&plaintext).expect("notification JSON");
    assert_eq!(
        notification["method"].as_str(),
        Some("notifications/strangecoin/event")
    );
    let params = &notification["params"];
    let event = &params["event"];
    assert_eq!(event["event_type"].as_str(), Some("BlockApplied"));
    assert_eq!(event["height"], tip.index);
    signing::verify(
        params["signer"].as_str().expect("event signer"),
        event,
        params["signature"].as_str().expect("event signature"),
    )
    .expect("per-event signature verifies (V-43)");
}

// ------------------------------------------------------------- КГ #3: signing

#[tokio::test]
async fn mcp_response_signature() {
    let (_dir, bc, alice, _bob) = seed_chain("mcp_sig");
    let server = server_for(&bc);
    let ctx = RequestCtx::new(TransportKind::Stdio);

    let response = server
        .handle(
            json!({
                "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": {"name": "get_balance", "arguments": {"address": alice}}
            }),
            &ctx,
        )
        .expect("tools/call responds");

    // Every response carries signature + signer (V-43).
    assert_eq!(
        response["signer"].as_str(),
        Some(hex::encode(server.signer().public_key()).as_str()),
        "signer is the node identity"
    );
    assert!(
        response["signature"].as_str().unwrap().len() == 128,
        "128 hex chars"
    );
    signing::verify_envelope(&response).expect("valid signature verifies");

    // Tampered payload → verification fails.
    let mut tampered = response.clone();
    tampered["result"]["content"][0]["text"] = json!("\"1\"");
    assert!(
        signing::verify_envelope(&tampered).is_err(),
        "tampered result rejected"
    );

    // Unsigned → rejected.
    let mut unsigned = response.clone();
    unsigned.as_object_mut().unwrap().remove("signature");
    assert!(
        signing::verify_envelope(&unsigned).is_err(),
        "unsigned rejected"
    );

    // Wrong key → rejected.
    let mut wrong_key = response.clone();
    wrong_key["signer"] = json!(hex::encode(NodeKeypair::from_seed([9u8; 32]).public_key()));
    assert!(
        signing::verify_envelope(&wrong_key).is_err(),
        "wrong key rejected"
    );
}

// ------------------------------------------------------- spec §9: determinism

#[test]
fn mcp_determinism_3_nodes() {
    let (_source_dir, source, alice, _bob) = seed_chain("mcp_det");
    let chain = source.chain_snapshot();
    let nodes = adopt_nodes(&chain, "mcp_det");
    let ctx = RequestCtx::new(TransportKind::Stdio);

    let requests = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
               "params":{"name":"get_block","arguments":{"height":2}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
               "params":{"name":"get_balance","arguments":{"address":alice}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
               "params":{"name":"estimate_fee","arguments":{"tx":{"type":"transfer"}}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"resources/read",
               "params":{"uri":"chain://tip"}}),
    ];

    for request in requests {
        let mut encoded: Vec<Vec<u8>> = Vec::new();
        for (_, bc) in &nodes {
            let server = server_for(bc);
            let response = server.handle(request.clone(), &ctx).expect("responds");
            encoded.push(serde_json::to_vec(&response).expect("response encodes"));
        }
        assert_eq!(
            encoded[0], encoded[1],
            "node outputs byte-identical: {request}"
        );
        assert_eq!(
            encoded[1], encoded[2],
            "node outputs byte-identical: {request}"
        );
    }

    // State really is identical: alice was granted 10000 and spent 500.
    let server = server_for(&nodes[0].1);
    let balance = server
        .handle(
            json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                   "params":{"name":"get_balance","arguments":{"address":alice}}}),
            &ctx,
        )
        .unwrap();
    let text = balance["result"]["content"][0]["text"].as_str().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["confirmed"], "9500");
    assert_eq!(value["pending"], "9500");
}

// ---------------------------------------------------- spec §9: resource: tip

#[test]
fn mcp_resource_chain_tip() {
    let (_dir, bc, _alice, _bob) = seed_chain("mcp_tip");
    let tip = bc.tip().expect("seeded chain has a tip");
    let server = server_for(&bc);
    let ctx = RequestCtx::new(TransportKind::Stdio);

    let response = server
        .handle(
            json!({"jsonrpc":"2.0","id":1,"method":"resources/read",
                   "params":{"uri":"chain://tip"}}),
            &ctx,
        )
        .expect("resources/read responds");
    signing::verify_envelope(&response).expect("signed");
    let contents = &response["result"]["contents"][0];
    assert_eq!(contents["uri"].as_str(), Some("chain://tip"));
    assert_eq!(contents["mimeType"].as_str(), Some("application/json"));
    let header: Value =
        serde_json::from_str(contents["text"].as_str().expect("text")).expect("header JSON");
    assert_eq!(header["height"], tip.index);
    assert_eq!(header["hash"].as_str(), Some(tip.hash.as_str()));
}

// ------------------------------------------------- spec §9: mcp-cli sequence

#[tokio::test]
async fn mcp_lifecycle_mcp_cli_sequence() {
    let (_dir, bc, _alice, _bob) = seed_chain("mcp_lifecycle");
    let mut client = Stdio::spawn(server_for(&bc));

    // initialize → negotiated pinned version + capabilities.
    let init = client
        .call(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                        "clientInfo": {"name": "mcp-cli", "version": "0"}}
        }))
        .await;
    let init: Value = serde_json::from_str(&init).unwrap();
    signing::verify_envelope(&init).expect("initialize is signed");
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(init["result"]["serverInfo"]["name"], "strangecoin-mcp");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    // initialized notification → no reply; the next line must be the
    // tools/list response (proves notifications are answer-free).
    client
        .send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await;
    let tools = client
        .call(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .await;
    let tools: Value = serde_json::from_str(&tools).unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(
        names,
        vec![
            "get_block",
            "get_balance",
            "get_nonce",
            "submit_tx",
            "call_contract",
            "estimate_fee",
            "subscribe_events",
        ],
        "7 tools in spec order"
    );

    let resources = client
        .call(json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}))
        .await;
    let resources: Value = serde_json::from_str(&resources).unwrap();
    let uris: Vec<&str> = resources["result"]["resources"]
        .as_array()
        .expect("resources array")
        .iter()
        .map(|r| r["uri"].as_str().expect("uri"))
        .collect();
    assert_eq!(
        uris,
        vec![
            "chain://tip",
            "chain://block/{height_or_hash}",
            "chain://tx/{txid}",
            "chain://contract/{address}",
        ],
        "4 resources in spec order"
    );

    let read = client
        .call(
            json!({"jsonrpc": "2.0", "id": 4, "method": "resources/read",
                      "params": {"uri": "chain://tip"}}),
        )
        .await;
    let read: Value = serde_json::from_str(&read).unwrap();
    signing::verify_envelope(&read).expect("resource read is signed");
    assert!(read["result"]["contents"][0]["text"].as_str().is_some());

    // shutdown → ack, then the serve loop exits (EOF not required).
    let bye = client
        .call(json!({"jsonrpc": "2.0", "id": 5, "method": "shutdown"}))
        .await;
    let bye: Value = serde_json::from_str(&bye).unwrap();
    assert!(bye.get("result").is_some());
    client.shutdown().await;
}

// -------------------------------------------------- КГ #4: reference mcp-cli

/// Round-trip through the real reference client (`uvx mcp-cli`, PyPI
/// `mcp-cli` — documented setup in `docs/spec/mcp_server.md` §9).
///
/// Env-gated on `STRANGECOIN_MCP_CLI_TEST=1`: the first run downloads the
/// package from PyPI, so the test would be network-dependent/flaky in CI.
/// Everything the client exercises is always covered by
/// [`mcp_lifecycle_mcp_cli_sequence`] above; this test adds the real
/// third-party client on top.
#[tokio::test]
async fn mcp_cli_reference_compat() {
    if std::env::var("STRANGECOIN_MCP_CLI_TEST").as_deref() != Ok("1") {
        eprintln!(
            "mcp_cli_reference_compat skipped: set STRANGECOIN_MCP_CLI_TEST=1 to run it \
             (needs `uvx` on PATH + network for the first `uvx mcp-cli` download; setup in \
             docs/spec/mcp_server.md §9)"
        );
        return;
    }

    let (dir, bc, _alice, _bob) = seed_chain("mcp_cli_ref");
    // The node binary opens the chain exclusively — release the lock first.
    drop(bc);

    let exe = env!("CARGO_BIN_EXE_strangecoin");
    let data_dir = dir.path().to_str().expect("utf-8 temp path");
    let config_path = dir.path().join("server_config.json");
    let config = json!({
        "mcpServers": {
            "strangecoin": {
                "command": exe,
                "args": ["--mcp-stdio", "--mcp-data-dir", data_dir],
                "env": {}
            }
        }
    });
    std::fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("config"),
    )
    .expect("write server_config.json");

    // mcp-cli discovers `server_config.json` in its CWD (documented
    // config-discovery rule — the explicit `--config-file` flag is not
    // honored before the subcommand in the published releases).
    let output = std::process::Command::new("uvx")
        .args(["mcp-cli", "tools", "--server", "strangecoin"])
        .current_dir(dir.path())
        .output()
        .expect("uvx mcp-cli must be runnable (setup: docs/spec/mcp_server.md §9)");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "mcp-cli exited non-zero\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    for tool in [
        "get_block",
        "get_balance",
        "get_nonce",
        "submit_tx",
        "call_contract",
        "estimate_fee",
        "subscribe_events",
    ] {
        assert!(
            stdout.contains(tool),
            "mcp-cli listing misses `{tool}`\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}
