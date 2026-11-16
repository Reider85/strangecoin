# Strangecoin MCP (Model Context Protocol) Server Spec

**Status:** Proposed (2026-11-04)
**Implementation prompt:** `analytics/prompt-stage3.md` §9 `AI-P02`
**Authoritative ADR:** [ADR-0029](../ADR/0029-mcp-server.md)
**Related specs:** [agent_integration.md](agent_integration.md), [acp_messages.md](acp_messages.md)
**Invariants enforced:** #23 (MCP tool output determinism)
**Threat vectors mitigated:** V-43 (MCP tool poisoning), V-46 (Agent key theft via stdio)
**MCP spec version:** 2024-11-05 (pinned; upgrade requires ADR amendment)
**Spec version:** 1.0

---

## 1. Overview

Strangecoin exposes itself as an **MCP (Model Context Protocol) server** so
that any MCP-capable LLM (Claude, GPT, Gemini, open-weight via `mcp-cli`)
can read chain state and submit transactions without a custom plugin.

This spec defines:

- The **7 tools** (callable functions) the server exposes.
- The **4 resources** (read-only URIs) the server exposes.
- The **2 transports** (stdio + HTTP+SSE) and their auth.
- The **output determinism** requirement (invariant #23).
- The **response signing** mechanism (V-43 mitigation).
- The **MCP lifecycle** the server implements.

MCP is a **parallel facade** to JSON-RPC `eth_*` — both ship in Stage 4.
MCP serves LLM tool-calling consumers; `eth_*` serves Ethereum tooling
(MetaMask, WalletConnect).

---

## 2. Tools (7)

Tools are callable functions exposed via `tools/list` and invoked via
`tools/call`. Each tool has a JSON schema (input + output).

### 2.1. `get_block`

```json
{
  "name": "get_block",
  "description": "Get a block by height or hash.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "height": {"type": "integer", "minimum": 0},
      "hash": {"type": "string", "pattern": "^[0-9a-f]{64}$"}
    },
    "oneOf": [{"required": ["height"]}, {"required": ["hash"]}]
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "height": {"type": "integer"},
      "hash": {"type": "string"},
      "prev_hash": {"type": "string"},
      "timestamp": {"type": "integer"},
      "tx_root": {"type": "string"},
      "state_root": {"type": "string"},
      "receipts_root": {"type": "string"},
      "txs": {"type": "array", "items": {"type": "object"}}
    }
  }
}
```

**Determinism (#23):** output is a pure function of chain state (blocks CF).
Same `height` → byte-identical JSON on every node at the same chain tip.

### 2.2. `get_balance`

```json
{
  "name": "get_balance",
  "description": "Get confirmed and pending balance for an address.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "address": {"type": "string", "pattern": "^rsc1[a-z0-9]{39}$"}
    },
    "required": ["address"]
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "confirmed": {"type": "string", "description": "atomic units, string to avoid precision loss"},
      "pending": {"type": "string"}
    }
  }
}
```

- `confirmed` — balance from `state` CF at current tip.
- `pending` — balance + pending mempool txs (incoming - outgoing).
- Address validation: bech32 with HRP `rsc1` (regtest), `tsc1` (testnet),
  `sc1` (mainnet).

### 2.3. `get_nonce`

```json
{
  "name": "get_nonce",
  "description": "Get the current nonce (tx count) for an address.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "address": {"type": "string", "pattern": "^rsc1[a-z0-9]{39}$"}
    },
    "required": ["address"]
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "nonce": {"type": "integer"}
    }
  }
}
```

### 2.4. `submit_tx`

```json
{
  "name": "submit_tx",
  "description": "Submit a signed transaction to the mempool.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "signed_tx_bytes": {"type": "string", "description": "hex-encoded canonical signed tx bytes"}
    },
    "required": ["signed_tx_bytes"]
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "txid": {"type": "string"},
      "status": {"type": "string", "enum": ["queued", "rejected"]},
      "reason": {"type": "string", "description": "present if status=rejected"}
    }
  }
}
```

- Server validates signature, checks mempool policy (RBF, double-spend, gas
  minimum), queues or rejects.
- `txid` is `blake3(canonical_signed_tx_bytes)` — deterministic from input.

### 2.5. `call_contract`

```json
{
  "name": "call_contract",
  "description": "Read-only contract call (eth_call equivalent). Does not modify state.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "address": {"type": "string", "pattern": "^rsc1[a-z0-9]{39}$"},
      "method": {"type": "string", "description": "4-byte blake3 method selector (hex) or ABI method name"},
      "args": {"type": "array"}
    },
    "required": ["address", "method", "args"]
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "result": {"type": "string", "description": "hex-encoded return bytes"},
      "gas_used": {"type": "integer"},
      "logs": {"type": "array", "items": {"type": "object"}}
    }
  }
}
```

- Executes via Stage 1.5 `wasmi` VM in read-only mode (`staticcall`).
- Does not consume on-chain nonce or gas.

### 2.6. `estimate_fee`

```json
{
  "name": "estimate_fee",
  "description": "Estimate gas and fee for a transaction. Stage 4 uses fixed gas price; Stage 5 will use EIP-1559.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "tx": {
        "type": "object",
        "properties": {
          "type": {"type": "string", "enum": ["transfer", "call", "create"]},
          "to": {"type": "string"},
          "amount": {"type": "string"},
          "method": {"type": "string"},
          "args": {"type": "array"}
        },
        "required": ["type"]
      }
    },
    "required": ["tx"]
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "gas_units": {"type": "integer"},
      "fee_sc": {"type": "string"}
    }
  }
}
```

### 2.7. `subscribe_events`

```json
{
  "name": "subscribe_events",
  "description": "Subscribe to chain events via SSE stream.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "filter": {
        "type": "object",
        "properties": {
          "event_types": {"type": "array", "items": {"type": "string", "enum": ["BlockApplied", "TxConfirmed", "EventLog"]}},
          "addresses": {"type": "array", "items": {"type": "string"}},
          "event_sigs": {"type": "array", "items": {"type": "string"}}
        }
      }
    }
  },
  "outputSchema": {
    "type": "object",
    "description": "SSE stream — each event is one JSON object",
    "properties": {
      "event_type": {"type": "string"},
      "data": {"type": "object"}
    }
  }
}
```

- Returns an SSE stream (HTTP+SSE transport only).
- Event content per event is deterministic; timing is not (event-driven).
- `BlockApplied` — fires on every new block applied to chain.
- `TxConfirmed` — fires when a tx is included in a block.
- `EventLog` — fires on contract event logs (filtered by `event_sigs` and
  `addresses`).

---

## 3. Resources (4)

Resources are read-only URIs exposed via `resources/list` and read via
`resources/read`. URI scheme: `chain://`.

### 3.1. `chain://tip`

Returns the latest block header (same as `get_block` output but header
only — no txs).

### 3.2. `chain://block/{height_or_hash}`

Returns a full block (header + txs). `{height_or_hash}` is either an
integer height or a 64-char hex hash.

### 3.3. `chain://tx/{txid}`

Returns a transaction + its receipt (status, gas used, logs).

### 3.4. `chain://contract/{address}`

Returns contract info: code (WASM bytes, base64), storage root, ABI (JSON).

---

## 4. Transports

### 4.1. stdio (default, local-only, no auth)

The MCP server reads JSON-RPC 2.0 requests from stdin (newline-delimited)
and writes responses to stdout. Suitable for IDE plugins (Claude Code,
Cursor) that spawn the Strangecoin node as a child process.

```
Client (IDE plugin)
    │
    │ spawn: strangecoin --mcp-stdio
    │
    ▼
Strangecoin node (child process)
    │
    │ stdin  → JSON-RPC requests (newline-delimited)
    │ stdout → JSON-RPC responses + SSE stream for subscribe_events
    │
    ▼
Chain state (RocksDB)
```

**Security:** relies on process isolation. The IDE plugin spawns the node
with restricted capabilities (no network access needed if using local
regtest chain). No auth required — the parent process is trusted.

### 4.2. HTTP+SSE (remote-capable, requires auth)

The MCP server runs as an HTTP endpoint:

- `POST /mcp` — JSON-RPC 2.0 request/response (for tools/call,
  tools/list, resources/read, etc.)
- `GET /mcp/events` — SSE stream (for `subscribe_events`)

**Auth:** Noise-XX handshake (ADR-0020) reuse from `strangecoin-net::noise`.

```
Client (remote agent runtime)
    │
    │ 1. Noise-XX handshake (agent presents node_id or ephemeral key
    │    derived from agent keypair per ADR-0028)
    │
    ▼
Strangecoin node (HTTP+SSE server)
    │
    │ 2. ChaCha20-Poly1305 encrypted transport
    │
    │ POST /mcp  → JSON-RPC requests
    │ GET  /mcp/events → SSE stream
    │
    ▼
Chain state
```

**Config:**

```toml
[mcp]
enabled = true
transport = "both"  # "stdio" | "http-sse" | "both"
http_listen = "127.0.0.1:9001"  # bind addr for HTTP+SSE
```

---

## 5. Output determinism (invariant #23)

Every tool output (except `subscribe_events` stream content, which is
event-driven) is a pure function of chain state:

- **No `SystemTime::now()` calls** in tool handlers.
- **No random values.**
- **No peer state** (peer list, sync status — these are operator-only,
  exposed in UI dashboard UI-P04, not in MCP).
- **Canonical serialization:** sorted JSON keys, deterministic byte order
  for binary fields (hex-encoded).
- **Same `get_block(h)` returns byte-identical JSON on every node** at the
  same chain height.

### 5.1. Enforcement

- **Pure-core reuse:** MCP handler is a thin shell over `strangecoin-core`
  types (`Block`, `AccountState`, `Transaction`, `Receipt`). All business
  logic stays in pure-core.
- **CI fuzz target `mcp_determinism`:** N=3 nodes started from the same
  snapshot, `get_block(h)` called on each, responses byte-compared.
  `cargo +nightly fuzz run mcp_determinism -- -max_total_time=600`.
- **CI gate:** `cargo test -p strangecoin-mcp --features determinism-fuzz`
  must pass on every PR.

### 5.2. Non-deterministic fields (excluded from tools)

These are operator-only, surfaced via UI dashboard (UI-P04), not via MCP:

- Peer list (peer_id, addr, latency, score).
- Sync status (current height, tip height, queue depth).
- Mempool contents (top-N by fee).
- Node config (active features, enabled transports).

---

## 6. Response signing (V-43 mitigation)

Every MCP response carries a `signature` field:

```json
{
  "result": { ... tool output ... },
  "signature": "0x...128_hex_chars_Ed25519_over_canonical_json(result)...",
  "signer": "0x...64_hex_chars_node_id_Ed25519_pubkey..."
}
```

- `signature` = `Ed25519(node_id_privkey, canonical_json(result))`.
- `signer` = node's `node_id` Ed25519 pubkey (advertised in Stage 2 `HELLO`
  message).
- Client verifies signature against `signer` before trusting the result.
- Unsigned responses are rejected by spec-compliant clients.

This mitigates V-43 (MCP tool poisoning) — a MitM cannot forge responses
without the node's Ed25519 key, even if it controls the transport.

---

## 7. MCP lifecycle

The server implements the standard MCP lifecycle (spec 2024-11-05):

1. **`initialize`** — client sends capabilities; server responds with its
   capabilities (tools, resources, prompts-disabled, version).
2. **`initialized`** — client confirms initialization complete.
3. **`tools/list`** — client requests tool list; server returns 7 tool
   schemas.
4. **`tools/call`** — client invokes a tool with args; server returns
   result (signed).
5. **`resources/list`** — client requests resource URIs; server returns 4
   resource URI templates.
6. **`resources/read`** — client reads a resource by URI; server returns
   content (signed).
7. **`notifications/subscribe`** (for `subscribe_events`) — client
   subscribes to event stream; server sends SSE events (per-event signed).
8. **`shutdown`** — client requests server shutdown; server cleans up.

---

## 8. Crate structure

```
crates/strangecoin-mcp/
├── Cargo.toml
├── src/
│   ├── lib.rs           # McpBackend trait, McpServer (JSON-RPC 2.0 handler)
│   ├── stdio.rs         # stdio transport
│   ├── http_sse.rs      # HTTP+SSE transport (axum or hyper)
│   ├── auth.rs          # Noise-XX reuse for HTTP+SSE
│   ├── signing.rs       # Ed25519 response signing + verification helpers
│   ├── tools/
│   │   ├── get_block.rs
│   │   ├── get_balance.rs
│   │   ├── get_nonce.rs
│   │   ├── submit_tx.rs
│   │   ├── call_contract.rs
│   │   ├── estimate_fee.rs
│   │   └── subscribe_events.rs
│   └── resources/
│       ├── tip.rs
│       ├── block.rs
│       ├── tx.rs
│       └── contract.rs
└── tests/
    └── mcp_server.rs    # round-trip + determinism + signature tests
```

**Dependencies:**

- `strangecoin-core = { path = "../strangecoin-core" }` (pure types)
- `strangecoin-storage = { path = "../strangecoin-storage" }` (StorageEngine for lookups)
- `serde_json = "=x.y.z"` (canonical serialization; pinned per Stage 3 precedent)
- `tokio` (async runtime for SSE)
- `snow = "=x.y.z"` (Noise-XX reuse from Stage 2)
- `axum` or `hyper` (HTTP+SSE server)
- `ed25519-dalek` (response signing; same crate as Stage 2 node_id)

---

## 9. Test plan

`tests/mcp_server.rs`:

| Test | What it verifies |
|------|------------------|
| `mcp_stdio_get_block_round_trip` | stdio transport, `tools/call get_block(1)` → JSON response, byte-identical across N=3 nodes (#23) |
| `mcp_http_sse_auth` | HTTP+SSE requires Noise-XX; reject unauthenticated requests |
| `mcp_response_signature` | Every response carries Ed25519 signature; client verifies (V-43 mitigation) |
| `mcp_cli_reference_compat` | Round-trip with reference `mcp-cli` client (documented setup) |
| `mcp_determinism_3_nodes` | N=3 nodes started from same snapshot; `get_block(h)` byte-identical on all 3 |
| `mcp_subscribe_events_sse` | `subscribe_events` SSE stream delivers `BlockApplied` events on new blocks |
| `mcp_resource_chain_tip` | `resources/read chain://tip` returns latest block header |

CI gates:

- `cargo test -p strangecoin-mcp` (all tests green)
- `cargo clippy -p strangecoin-mcp -- -D warnings` (green)
- `cargo +nightly fuzz run mcp_determinism -- -max_total_time=600` (no panic)

---

## 10. Anti-goals

- **MCP server does not replace JSON-RPC `eth_*`.** Both ship in Stage 4;
  MCP for LLM consumers, `eth_*` for Ethereum tooling.
- **MCP server does not expose operator-only data.** Peer list, sync
  status, mempool contents, node config — these are surfaced in UI dashboard
  (UI-P04), not via MCP.
- **MCP server does not implement `prompts/*` MCP primitive.** Stage 4
  ships tools + resources only; prompts are optional in MCP and deferred
  to Stage 5+ if needed.
- **MCP server does not implement WebSocket transport.** Spec 2024-11-05
  fixes stdio + HTTP+SSE; WebSocket is non-spec.

---

## 11. References

- [ADR-0029](../ADR/0029-mcp-server.md)
- [MCP](https://modelcontextprotocol.io)
- [MCP spec 2024-11-05](https://spec.modelcontextprotocol.io/spec/2024-11-05/)
- [mcp-cli](https://github.com/modelcontextprotocol/mcp-cli) — reference client
- [ADR-0020](../ADR/0020-noise-protocol-framework.md) — Noise-XX (reused for HTTP+SSE auth)
- [agent_integration.md](agent_integration.md) — agent identity (V-46 mitigation context)
- [ARCHITECT.md §19.3/§19.5/§19.6](../../analytics/ARCHITECT.md)
- [prompt-stage3.md §9 AI-P02](../../analytics/prompt-stage3.md)
