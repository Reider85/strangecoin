//! AI-P02 — extra determinism suite behind `--features determinism-fuzz`
//! (КГ: `cargo test --features determinism-fuzz`). The always-on real-facade
//! 3-node cross-check lives in the root `tests/mcp_server.rs`; this file
//! exercises the pure protocol layer over identical mock state, where the
//! serialization/signing pipeline can be byte-compared with zero chain
//! dependencies.

#![cfg(feature = "determinism-fuzz")]

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use strangecoin_core::types::{Block, BlockHeader, Transaction};
use strangecoin_mcp::{
    CallOutcome, ContractInfo, McpBackend, McpError, McpEvent, McpServer, RequestCtx,
    TransportKind, TxRecord,
};
use strangecoin_net::NodeKeypair;

fn mock_block(index: u64) -> Block {
    Block {
        index,
        timestamp: 1_700_000_000 + index,
        transactions: vec![],
        previous_hash: if index == 0 {
            "0".repeat(64)
        } else {
            format!("{:064x}", index - 1)
        },
        hash: format!("{index:064x}"),
        nonce: index,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [index as u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    }
}

struct MockBackend {
    blocks: Vec<Block>,
    balances: BTreeMap<String, u64>,
}

impl MockBackend {
    fn seeded() -> Self {
        let mut balances = BTreeMap::new();
        balances.insert("rsc1-address-placeholder".to_string(), 42);
        Self {
            blocks: (0..3).map(mock_block).collect(),
            balances,
        }
    }
}

impl McpBackend for MockBackend {
    fn tip_header(&self) -> Option<BlockHeader> {
        self.blocks.last().map(|b| b.header())
    }
    fn block_by_height(&self, height: u64) -> Option<Block> {
        usize::try_from(height)
            .ok()
            .and_then(|i| self.blocks.get(i))
            .cloned()
    }
    fn block_by_hash(&self, hash_hex: &str) -> Option<Block> {
        self.blocks.iter().find(|b| b.hash == hash_hex).cloned()
    }
    fn get_balance(&self, address: &str) -> u64 {
        self.balances.get(address).copied().unwrap_or(0)
    }
    fn get_nonce(&self, _: &str) -> u64 {
        0
    }
    fn pending_balance(&self, address: &str) -> u64 {
        self.get_balance(address)
    }
    fn submit_tx(&self, _: Transaction) -> Result<[u8; 32], McpError> {
        Err(McpError::Rejected("mock".into()))
    }
    fn call_contract(&self, _: &str, _: Vec<u8>, _: u64) -> Result<CallOutcome, McpError> {
        Err(McpError::NotFound("mock".into()))
    }
    fn get_tx_with_receipt(&self, txid: &str) -> Result<TxRecord, McpError> {
        Err(McpError::NotFound(format!("mock: {txid}")))
    }
    fn contract_info(&self, addr: &str) -> Result<ContractInfo, McpError> {
        Err(McpError::NotFound(format!("mock: {addr}")))
    }
    fn subscribe_events(&self) -> tokio::sync::mpsc::Receiver<McpEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(tx);
        rx
    }
}

fn three_servers() -> Vec<Arc<McpServer>> {
    let identity = NodeKeypair::from_seed([11u8; 32]);
    (0..3)
        .map(|_| {
            let backend: Arc<dyn McpBackend> = Arc::new(MockBackend::seeded());
            McpServer::new(backend, identity.clone())
        })
        .collect()
}

/// Every dispatch arm produces byte-identical signed envelopes across N=3.
#[test]
fn all_dispatch_arms_are_byte_identical_across_three_nodes() {
    let requests = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize",
               "params":{"protocolVersion":"2024-11-05"}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
        json!({"jsonrpc":"2.0","id":4,"method":"resources/templates/list"}),
        json!({"jsonrpc":"2.0","id":5,"method":"resources/read",
               "params":{"uri":"chain://tip"}}),
        json!({"jsonrpc":"2.0","id":6,"method":"resources/read",
               "params":{"uri":"chain://block/2"}}),
        json!({"jsonrpc":"2.0","id":7,"method":"resources/read",
               "params":{"uri":"chain://block/deadbeef"}}),
        json!({"jsonrpc":"2.0","id":8,"method":"tools/call",
               "params":{"name":"get_block","arguments":{"height":1}}}),
        json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
               "params":{"name":"get_balance","arguments":{"address":"rsc1-address-placeholder"}}}),
        json!({"jsonrpc":"2.0","id":10,"method":"tools/call",
               "params":{"name":"get_nonce","arguments":{"address":"garbage"}}}),
        json!({"jsonrpc":"2.0","id":11,"method":"tools/call",
               "params":{"name":"estimate_fee","arguments":{"tx":{"type":"transfer"}}}}),
        json!({"jsonrpc":"2.0","id":12,"method":"tools/call",
               "params":{"name":"subscribe_events","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":13,"method":"tools/call",
               "params":{"name":"no_such_tool","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":14,"method":"shutdown"}),
        json!({"jsonrpc":"2.0","id":15,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":16,"method":"totally/unknown"}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ];

    for request in requests {
        let mut encoded: Vec<Vec<u8>> = Vec::new();
        for server in three_servers() {
            let ctx = RequestCtx::new(TransportKind::Stdio);
            if let Some(response) = server.handle(request.clone(), &ctx) {
                strangecoin_mcp::signing::verify_envelope(&response)
                    .expect("signed envelope verifies");
                encoded.push(serde_json::to_vec(&response).expect("encodes"));
            }
        }
        // Notifications produce no response on every node alike; anything
        // that DID respond must match byte-for-byte (signed errors included).
        if !encoded.is_empty() {
            assert_eq!(encoded[0], encoded[1], "byte-identical: {request}");
            assert_eq!(encoded[1], encoded[2], "byte-identical: {request}");
        }
    }
}

/// Key identity is pinned: the same seed must sign the same bytes every
/// run (RFC 8032 determinism underpins the whole #23 cross-check).
#[test]
fn signatures_are_stable_across_server_instances() {
    let servers = three_servers();
    let ctx = RequestCtx::new(TransportKind::Stdio);
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let first = servers[0].handle(request.clone(), &ctx).unwrap();
    let second = servers[1].handle(request, &ctx).unwrap();
    assert_eq!(first["signature"], second["signature"]);
    assert_eq!(
        first["signer"].as_str(),
        Some(hex::encode(NodeKeypair::from_seed([11u8; 32]).public_key()).as_str())
    );
}
