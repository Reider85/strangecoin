# ADR-0029: MCP (Model Context Protocol) Server

## Status

Accepted (2026-11-18) — design ratified together with the land of the MCP
server scaffold in commit `[S4-AI-P02]` (`crates/strangecoin-mcp/`: 7 tools,
4 resources, stdio + HTTP+SSE transports, deterministic signed output,
`mcp-cli` round-trip). Originally Proposed (2026-11-04); the acceptance
gate (scaffold land) closed with that commit. Implementation-level
deviations from the original text are recorded in
[Implementation amendments](#implementation-amendments-ai-p02-land) below.

## Context

The Model Context Protocol (MCP, [modelcontextprotocol.io](https://modelcontextprotocol.io),
spec version 2024-11-05) is Anthropic's open standard for connecting LLMs to
external tools, resources, and prompts. It is a JSON-RPC 2.0 protocol with
three primitives:

- **Tools** — callable functions the LLM can invoke (with arguments, returns
  structured output).
- **Resources** — read-only URIs the LLM can read (text or binary content).
- **Prompts** — reusable prompt templates (optional; deferred for Stage 4).

MCP is supported by Claude (native), GPT (via MCP-bridge), Gemini, and
open-weight LLMs via `mcp-cli`. An MCP server can be exposed over **stdio**
(local, for LLM IDE plugins like Claude Code, Cursor) or **HTTP+SSE** (remote,
for cloud agents).

Strangecoin today exposes its state via two channels: a JSON-RPC surface
(`src/api/`, currently minimal — Stage 4 extends it with `eth_*` + `agent_*`)
and the `NodeEvent` bus (Stage 1, internal). Neither is MCP-compatible — an
LLM consumer would need a custom wrapper to call `eth_getBlockByNumber` and
parse the response into a tool-call result.

The forces in tension:

1. **LLM ecosystem interop vs. minimal scope.** Shipping an MCP server makes
   Strangecoin immediately usable by any MCP-capable LLM without per-vendor
   plugins. But it adds a new crate, a new transport, and a new spec to track.
2. **stdio vs. HTTP+SSE.** stdio is local-only (relies on process isolation
   for security) — perfect for IDE plugins. HTTP+SSE is remote-capable but
   requires authentication. Both are needed for different use cases.
3. **Output determinism.** MCP tool output must be deterministic across nodes
   (invariant #23) — same `get_block(h)` returns byte-identical JSON on every
   node. This rules out including local time, peer state, or random values in
   tool output.
4. **Tool poisoning threat (V-43).** A malicious MCP server could feed false
   state to an LLM. Mitigation: responses are signed by the node's Ed25519
   `node_id` key (Stage 2), client verifies.

## Decision

**We will implement an MCP server in a new crate
`crates/strangecoin-mcp/`, exposing Strangecoin chain state as 7 tools and 4
resources over two transports (stdio + HTTP+SSE), with deterministic output
and signed responses.**

### Tools (7)

| Tool | Signature | Description | Pure function of |
|------|-----------|-------------|------------------|
| `get_block` | `(height: u64 \| hash: [u8;32]) → BlockJSON` | Get a block by height or hash | chain state (blocks CF) |
| `get_balance` | `(address: bech32) → { confirmed: u64, pending: u64 }` | Get confirmed + pending balance | chain state (state CF + mempool) |
| `get_nonce` | `(address: bech32) → u64` | Get account nonce | chain state (state CF) |
| `submit_tx` | `(signed_tx_bytes: bytes) → { txid: [u8;32], status: "queued" \| "rejected", reason?: string }` | Submit a signed transaction to mempool | side-effecting; returns txid (deterministic from tx bytes) |
| `call_contract` | `(address: bech32, method: string, args: array) → { result: bytes, gas_used: u64, logs: array }` | Read-only contract call (eth_call equivalent) | chain state (contract_storage CF + VM) |
| `estimate_fee` | `(tx: UnsignedTxJSON) → { gas_units: u64, fee_sc: u64 }` | Estimate gas + fee for a tx (Stage 4 fixed gas price; Stage 5 EIP-1559) | chain state (gas estimator) |
| `subscribe_events` | `(filter: EventFilter) → SSE stream of EventJSON` | Subscribe to chain events (BlockApplied, TxConfirmed, EventLog) | live event bus (Stage 1); SSE stream (non-deterministic timing, deterministic content per event) |

### Resources (4)

| Resource URI | Content | Description |
|--------------|---------|-------------|
| `chain://tip` | BlockHeaderJSON | Latest block header |
| `chain://block/{height_or_hash}` | BlockJSON | Full block by height or hash |
| `chain://tx/{txid}` | TxWithReceiptJSON | Transaction + receipt |
| `chain://contract/{address}` | ContractInfoJSON | Contract code (WASM bytes, base64) + storage root + ABI |

### Transports

1. **stdio** (default, local-only, no auth). The MCP server reads JSON-RPC 2.0
   requests from stdin (newline-delimited) and writes responses to stdout.
   Security relies on process isolation — the LLM runtime spawns the Strangecoin
   node as a child process with restricted capabilities. Suitable for IDE
   plugins (Claude Code, Cursor).

2. **HTTP+SSE** (remote-capable, requires auth). The MCP server runs as an HTTP
   endpoint (`POST /mcp` for JSON-RPC requests, `GET /mcp/events` for SSE
   stream). Authentication reuses the Stage 2 Noise-XX handshake (ADR-0020):
   the agent presents its `node_id` (or an ephemeral key derived from its
   agent keypair per ADR-0028), the node responds with Noise-XX, the transport
   is then encrypted with ChaCha20-Poly1305.

### Output determinism (invariant #23)

Every tool output (except `subscribe_events` stream content, which is
event-driven) is a pure function of chain state:

- No `SystemTime::now()` calls in tool handlers.
- No random values.
- No peer state (peer list, sync status — these are operator-only, exposed in
  UI dashboard UI-P04, not in MCP).
- Same `get_block(h)` returns byte-identical JSON on every node at the same
  chain height.

Enforced by:

- Pure-core reuse: MCP handler is a thin shell over `strangecoin-core` types
  (`Block`, `AccountState`, `Transaction`, `Receipt`). Serialization is
  canonical (sorted keys, deterministic byte order).
- CI fuzz target `mcp_determinism`: N=3 nodes started from the same snapshot,
  `get_block(h)` called on each, responses byte-compared.
- `cargo test -p strangecoin-mcp --features determinism-fuzz` is a CI gate.

### Response signing (V-43 mitigation)

Every MCP response carries a `signature: Ed25519(node_id, canonical_json(response))`
field. The client verifies the signature against the node's advertised `node_id`
(Stage 2 `HELLO` message). Unsigned responses are rejected by spec-compliant
clients. This mitigates the V-43 threat (MCP tool poisoning) — a MitM cannot
forge responses without the node's Ed25519 key.

### MCP spec compliance

The server implements the MCP lifecycle:

- `initialize` → server responds with capabilities (tools, resources, prompts)
- `initialized` → client confirms
- `tools/list` → server returns tool schemas (JSON)
- `tools/call` → server invokes tool, returns result
- `resources/list` → server returns resource URIs
- `resources/read` → server returns resource content
- `notifications/*` → for `subscribe_events` SSE stream

Reference client `mcp-cli` is used for round-trip compatibility testing
(`tests/mcp_server.rs::mcp_cli_reference_compat`).

### Crate structure

```
crates/strangecoin-mcp/
├── Cargo.toml           # strangecoin-mcp = "0.1.0", features = ["stdio", "http-sse"]
├── src/
│   ├── lib.rs           # McpBackend trait, McpServer (JSON-RPC 2.0 handler)
│   ├── stdio.rs         # stdio transport
│   ├── http_sse.rs      # HTTP+SSE transport
│   ├── auth.rs          # Noise-XX reuse from strangecoin-net::noise
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

### Pin policy

- `snow` (Noise-XX) — pinned `=x.y.z` (Stage 2 precedent, ADR-0020).
  *Resolved:* `strangecoin-mcp` never names `snow` directly — Noise goes
  through `strangecoin-net`, which pins `snow = "=0.10.0"`; the pin is
  inherited transitively (see amendments).
- `serde_json` — pinned (Stage 3 storage precedent, ADR-0024).
  *Resolved:* `=1.0.154` (Cargo.lock resolution, 2026-11-18).
- `axum` or `hyper` (HTTP+SSE server) — pinned `=x.y.z`.
  *Resolved:* `axum = "=0.8.9"` (2026-11-18); test-only `tower = "=0.5.3"`,
  `http-body-util = "=0.1.5"`.
- MCP spec version — pinned to 2024-11-05; spec churn risk documented in ADR
  (amendment required to upgrade).

### Implementation amendments (AI-P02 land)

Deviations from the original (2026-11-04) text, recorded per FBP:

1. **`snow` not a direct dependency.** The handshake/crypto surface is
   reached through `strangecoin_net::{NoiseHandshake, NoiseTransport,
   NodeKeypair}` (ADR-0020 reuse); `snow = "=0.10.0"` is inherited from
   strangecoin-net's pin instead of being listed twice.
2. **`McpNode` not needed.** The trait impl is
   `impl McpBackend for BlockchainFacade` in the root crate
   (`src/blockchain/mcp_backend.rs`, feature `mcp`) — the exact strangler
   pattern of `impl BlockchainView for BlockchainFacade` (S2-P06).
3. **Integration tests live in root `tests/mcp_server.rs`**, not
   `crates/strangecoin-mcp/tests/` as the crate-structure diagram below
   shows: facade-based tests cannot live in the mcp crate (a dev-dependency
   on the root crate would invert the dependency direction). The crate's own
   `tests/determinism.rs` (feature `determinism-fuzz`) holds the
   mock-backend cross-check instead.
4. **`call_contract` accepts raw calldata / 4-byte selectors only.** ABI
   method names cannot be encoded because the node never persists ABIs
   (pre-existing gap: `WasmiExecutor::insert_abi` has no production call
   site); the spec §2.5 `method` field is therefore selector-hex (or the
   raw `calldata` override added to the schema). The frame runs with
   `read_only: true` (spec: staticcall semantics); `caller = callee` since
   the schema carries no `from`.
5. **`estimate_fee` → `fee_sc = gas_units × 0`.** Stage 4 has no fee market
   (`fee_market.rs` is a Stage 5 TODO; constructors default `fee = 0`), so
   the fixed gas price constant is 0 and the gas estimate is the useful
   half. `type=create` reports `BLOCK_GAS_LIMIT` as an upper bound (the
   schema carries no deploy code to dry-run).
6. **`TxConfirmed`/`EventLog` are derived events.** The root `NodeEvent`
   bus carries only `BlockApplied`; the backend derives tx confirmations
   from the applied block's transactions and event logs from
   `tip_receipts()` while the block is the tip. Receipts exist only for the
   tip block (ADR-0024 has no receipts CF) — `chain://tx/{txid}` reports
   `receipt: null` for older transactions with the confirmed `height`.
7. **`--mcp-data-dir <path>` CLI flag** (overrides `storage.path` +
   `data_dir`) — testability hook for the `mcp-cli` round-trip; spec §4.1
   spawn example unchanged.
8. **HTTP wire = Noise-XX over HTTP POSTs.** `POST /mcp/handshake`
   carries the three XX messages as base64 bodies; `POST /mcp` bodies are
   base64 `encrypt_frame` ciphertext under `X-Mcp-Session`. Consequences:
   one in-flight request per session (strict Noise nonce ordering), a
   *second* session for `GET /mcp/events` (POST responses and SSE events
   would otherwise interleave on one send-nonce sequence), and **standard
   MCP HTTP clients cannot connect** — stdio is the interop transport, as
   the Alternatives section already assumed. Every SSE `data:` line is an
   encrypted, signed notification.
9. **`subscribe_events` is transport-split.** stdio: ack + server-pushed
   `notifications/strangecoin/event` lines. HTTP+SSE: ack pointing at
   `/mcp/events`; the filter travels as `?types=&addresses=&event_sigs=`
   query params (a filter passed in the tool arguments applies on stdio).
10. **`resources/templates/list` is served** (same four URIs) as an
    extra MCP primitive, and each tool entry carries `outputSchema` — both
    are supersets of the 2024-11-05 wire shape.
11. **`mcp-cli` round-trip is env-gated** (`STRANGECOIN_MCP_CLI_TEST=1`,
    needs `uvx` + network for the package download); the same wire sequence
    runs always-on in `mcp_lifecycle_mcp_cli_sequence`.
12. **Numbering note.** References to invariant **#23** (MCP output
    determinism) and threat **V-43** (MCP tool poisoning) use the
    `ARCHITECT.md` §19.5/§19.6 extension numbering — distinct from the
    Stage 3 `docs/stage3/INVARIANTS_ENFORCED.md` #23 (schema-version guard)
    and `THREAT_MODEL.md` V-43 (snapshot forging).

## Consequences

### Positive

- **LLM ecosystem interop without custom plugins.** One MCP server
  implementation serves Claude, GPT, Gemini, and any `mcp-cli`-compatible
  client.
- **Pure-core reuse preserves determinism.** MCP handler is a thin shell;
  all business logic lives in `strangecoin-core` (Stage 1 invariant).
- **Noise-XX reuse for HTTP+SSE auth.** No new crypto; the Stage 2 transport
  stack is reused.
- **Parallel facade to JSON-RPC.** MCP and `eth_*` JSON-RPC coexist — agents
  pick the surface that fits (MCP for LLM tool-calling, JSON-RPC for
  programmatic loops).

### Negative

- **stdio is local-only.** Remote agents must use HTTP+SSE, which requires
  running a server (ops burden) and managing Noise-XX auth.
- **MCP spec churn risk.** Spec version 2024-11-05 may change; pinning is a
  stopgap. Mitigation: amendment to this ADR when spec stabilizes.
- **Determinism testing overhead.** The `mcp_determinism` fuzz target requires
  N-node cross-check infrastructure (shared snapshot, parallel nodes). CI time
  cost: ~60s per run.
- **Response signing adds 88 bytes per response.** Ed25519 signature overhead.
  Negligible for tool calls; relevant for high-frequency `subscribe_events`
  streams (could batch-sign per SSE chunk).

### Neutral

- **MCP is not a replacement for JSON-RPC `eth_*`.** MCP serves LLM consumers;
  `eth_*` serves Ethereum tooling (MetaMask, WalletConnect). Both ship in
  Stage 4.

## Alternatives

### Alternative 1: stdio-only transport

- **Pros:** Simplest; no network; no auth needed (process isolation).
- **Cons:** Does not serve remote agents (cloud-hosted LLMs, multi-agent
  networks with separate hosts).
- **Why not chosen:** Remote agent use case is core to the AI-agent vision
  (ADR-0028). HTTP+SSE is required.

### Alternative 2: HTTP+SSE-only transport

- **Pros:** Universal; one transport to maintain.
- **Cons:** IDE plugin use case (local LLM, low-friction) requires stdio.
  Forcing HTTP+SSE for local use adds auth friction.
- **Why not chosen:** stdio is the MCP default for local IDE plugins; not
  shipping it breaks Claude Code / Cursor integration.

### Alternative 3: Custom JSON-RPC schema (non-MCP)

- **Pros:** No MCP spec dependency; can optimize for Strangecoin-specific
  needs.
- **Cons:** Loses LLM ecosystem interop; each LLM vendor needs a custom
  plugin; high maintenance burden.
- **Why not chosen:** Anti-goal (ARCHITECT.md §19.7 — no custom LLM-plugin
  protocol per vendor).

### Alternative 4: MCP-over-WebSocket

- **Pros:** Bidirectional; lower latency than HTTP+SSE for `subscribe_events`.
- **Cons:** Not in MCP spec 2024-11-05 (which fixes stdio + HTTP+SSE);
  implementing non-spec transport fragments the ecosystem.
- **Why not chosen:** Spec compliance is more valuable than the latency
  improvement for Stage 4.

## Related

- [ADR-0009](0009-events-bus.md) — NodeEvent bus (Stage 1; reused for `subscribe_events`)
- [ADR-0020](0020-noise-protocol-framework.md) — Noise transport (reused for HTTP+SSE auth)
- [ADR-0024](0024-storage-schema-and-engine.md) — Storage schema (Stage 3; provides `StorageEngine::get_tx` for resource lookups)
- [ADR-0028](0028-strangecoin-as-cryptocurrency-for-ai-agents.md) — AI agent vision (this ADR operationalizes it for MCP)
- [ADR-0030](0030-acp-agent-communication-protocol.md) — ACP (complementary agent-to-agent transport)
- [ADR-0031](0031-professional-ui-replacement.md) — Professional UI (Settings screen toggles MCP)
- [ARCHITECT.md §19.3](../../analytics/ARCHITECT.md) — MCP server subsystem
- [docs/spec/mcp_server.md](../spec/mcp_server.md) — MCP server spec (tool schemas, resource URIs, transport details, auth flow, determinism, response signing, lifecycle)
- [Model Context Protocol](https://modelcontextprotocol.io) — MCP standard
- [MCP spec 2024-11-05](https://spec.modelcontextprotocol.io/spec/2024-11-05/) — pinned spec version
- [mcp-cli](https://github.com/chrishayuk/mcp-cli) — reference client used
  for the round-trip test (PyPI `mcp-cli`, installed via `uvx`; the
  original `modelcontextprotocol/mcp-cli` link 404s)
