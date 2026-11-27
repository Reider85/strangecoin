//! AI-P02 — MCP output determinism fuzzing (invariant #23, ADR-0029).
//!
//! Contract: the same request bytes processed by N=3 independent MCP
//! servers over identical mock chain state produce **byte-identical**
//! signed envelopes (signature included — Ed25519 is deterministic per
//! key), and every envelope verifies against the shared signer. A panic is
//! a crash; a typed `McpError`/JSON-RPC error envelope is a pass.
//!
//! The backend is a static in-memory mock: no RocksDB, no runtime — the
//! fuzz budget goes entirely through the protocol layer (dispatch,
//! serialization, signing).

#![no_main]

use std::collections::BTreeMap;
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;
use serde_json::{json, Value};
use strangecoin_core::types::{Block, BlockHeader, Transaction};
use strangecoin_mcp::{
    CallOutcome, ContractInfo, McpBackend, McpError, McpEvent, McpServer, RequestCtx,
    TransportKind, TxRecord,
};
use strangecoin_net::NodeKeypair;

/// Two deterministic mock blocks (heights 0..=1).
fn mock_blocks() -> Vec<Block> {
    let make = |index: u64, prev: &str| Block {
        index,
        timestamp: 1_700_000_000 + index,
        transactions: vec![],
        previous_hash: prev.to_string(),
        hash: format!("{index:064x}"),
        nonce: index,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [index as u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    vec![make(0, &"0".repeat(64)), make(1, &format!("{:064x}", 0))]
}

struct MockBackend {
    blocks: Vec<Block>,
    balances: BTreeMap<String, u64>,
}

impl MockBackend {
    fn new() -> Self {
        let mut balances = BTreeMap::new();
        balances.insert("rsc1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_string(), 42);
        Self {
            blocks: mock_blocks(),
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

    fn get_nonce(&self, _address: &str) -> u64 {
        0
    }

    fn pending_balance(&self, address: &str) -> u64 {
        self.get_balance(address)
    }

    fn submit_tx(&self, _tx: Transaction) -> Result<[u8; 32], McpError> {
        Err(McpError::Rejected("mock backend does not accept txs".into()))
    }

    fn call_contract(
        &self,
        _address: &str,
        _calldata: Vec<u8>,
        _gas_limit: u64,
    ) -> Result<CallOutcome, McpError> {
        Err(McpError::NotFound("mock has no contracts".into()))
    }

    fn get_tx_with_receipt(&self, txid_hex: &str) -> Result<TxRecord, McpError> {
        Err(McpError::NotFound(format!("mock has no tx `{txid_hex}`")))
    }

    fn contract_info(&self, address: &str) -> Result<ContractInfo, McpError> {
        Err(McpError::NotFound(format!("mock has no contract `{address}`")))
    }

    fn subscribe_events(&self) -> tokio::sync::mpsc::Receiver<McpEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(tx); // no events — the receiver closes immediately
        rx
    }
}

fn run_case(data: &[u8]) {
    let identity = NodeKeypair::from_seed([7u8; 32]);
    let servers: Vec<Arc<McpServer>> = (0..3)
        .map(|_| {
            let backend: Arc<dyn McpBackend> = Arc::new(MockBackend::new());
            McpServer::new(backend, identity.clone())
        })
        .collect();

    let request = build_request(data);
    let ctx = RequestCtx::new(TransportKind::Stdio);

    let mut encoded: Vec<Vec<u8>> = Vec::with_capacity(3);
    for server in &servers {
        let response = server.handle(request.clone(), &ctx).expect("handle never panics");
        strangecoin_mcp::signing::verify_envelope(&response)
            .expect("every response carries a valid signature");
        encoded.push(serde_json::to_vec(&response).expect("response serializes"));
    }
    assert_eq!(encoded[0], encoded[1], "N=3 outputs must be byte-identical");
    assert_eq!(encoded[1], encoded[2], "N=3 outputs must be byte-identical");
}

/// Map fuzz bytes onto a request covering every dispatch arm (tools,
/// resources, lifecycle, malformed input).
fn build_request(data: &[u8]) -> Value {
    let pick = data.first().copied().unwrap_or(0);
    let rest = data.get(1..).unwrap_or(&[]);
    match pick % 9 {
        0 => json!({"jsonrpc":"2.0","id":1,"method":"initialize",
                     "params":{"protocolVersion":"2024-11-05"}}),
        1 => json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        2 => json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}),
        3 => json!({"jsonrpc":"2.0","id":1,"method":"resources/read",
                     "params":{"uri":String::from_utf8_lossy(rest).to_string()}}),
        4 => {
            let height_bytes: [u8; 8] = rest
                .get(..8)
                .and_then(|slice| slice.try_into().ok())
                .unwrap_or([0; 8]);
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                   "params":{"name":"get_block",
                             "arguments":{"height": u64::from_be_bytes(height_bytes) % 4}}})
        }
        5 => json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                     "params":{"name":"get_balance",
                               "arguments":{"address":String::from_utf8_lossy(rest).to_string()}}}),
        6 => {
            let kinds = ["transfer", "call", "create", "bogus"];
            let kind = kinds[usize::from(rest.first().copied().unwrap_or(0)) % 4];
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                   "params":{"name":"estimate_fee","arguments":{"tx":{"type":kind}}}})
        }
        7 => json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                     "params":{"name":"subscribe_events","arguments":{}}}),
        _ => {
            // Malformed method / raw fuzz text — must produce a signed
            // error envelope, never a panic.
            json!({"jsonrpc":"2.0","id":1,"method":String::from_utf8_lossy(rest).to_string()})
        }
    }
}

fuzz_target!(|data: &[u8]| {
    run_case(data);
});
