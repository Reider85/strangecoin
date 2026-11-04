# ADR-0030: ACP (Agent Communication Protocol)

## Status

Proposed (2026-11-04) — `AI-P03` (extension prompt in `analytics/prompt-stage3.md` §9). 
Acceptance gate: land of ACP wire messages (`ACP_NEGOTIATE` / `ACP_TASK` / `ACP_SETTLE` / `ACP_REVOKE`), `AcpHandler`, `EscrowVault` WASM contract, and `tests/acp_messages.rs` end-to-end test.

## Context

Stage 2 delivered a production-grade P2P transport: Noise XX handshake
(ADR-0020) + ChaCha20-Poly1305 encrypted transport + Ed25519 `node_id`
authentication + peer scoring (ADR-0022) + rate limiting (ADR-0023) + Erlay
set reconciliation. This transport carries blockchain messages (`VERSION`,
`VERACK`, `HELLO`, `ADDR`, `GET_HEADERS`, `HEADERS`, `GET_BLOCKS`, `BLOCKS`,
compact-blocks, Erlay sketches).

Strangecoin is positioned as a cryptocurrency for AI agents (ADR-0028). Agents
need to talk to each other — not just to the chain. The use cases:

1. **Task delegation.** Agent A wants agent B to perform a task (e.g., summarize
   a document, run a sandboxed analysis). A negotiates with B (quote, deadline),
   assigns the task, escrows payment, settles on completion.
2. **Micropayments.** Agent A pays agent B small amounts (e.g., 0.001 SC per
   API call) without on-chain tx per payment (gas cost would dominate).
3. **Multi-agent orchestration.** Agent A → B → C task chain (e.g., A
   commissions B, B sub-contracts C). Each leg has its own escrow.

The forces in tension:

1. **Centralized marketplace vs. peer-to-peer.** A centralized marketplace
   (OpenAI-style broker, LangHub, etc.) is simpler but introduces a trusted
   intermediary, fees, and a single point of failure. Peer-to-peer (agents
   negotiate directly) eliminates the intermediary but requires a wire
   protocol.
2. **On-chain escrow vs. off-chain.** On-chain escrow (WASM contract holding
   funds until settle/revoke) is publicly verifiable but costs gas per
   deposit/settle. Off-chain (HTLC-style or trust-based) is cheaper but
   requires indexer trust for verification.
3. **Micropayment feasibility.** Per-message on-chain tx is infeasible for
   sub-cent payments (gas cost). ACP messages must be off-chain; only the
   escrow deposit + settle/revoke go on-chain (2 txs per task, not N).
4. **Replay protection.** ACP messages are signed; an attacker who captures
   a message could replay it. Nonces must be tracked per `(agent_id,
   counterparty)` pair.
5. **Spam.** An attacker could flood `ACP_NEGOTIATE` messages to exhaust a
   peer. Rate limiting must apply.

## Decision

**We will implement ACP as a wire message family (`ACP_NEGOTIATE`,
`ACP_TASK`, `ACP_SETTLE`, `ACP_REVOKE`) riding on the existing Noise-
authenticated P2P transport, with on-chain escrow (WASM `EscrowVault`
contract), replay protection via monotonic per-counterparty nonces, and
rate limiting via the Stage 2 token-bucket.**

### Message family (4)

| Message | Direction | Purpose | Fields |
|---------|-----------|---------|--------|
| `ACP_NEGOTIATE` | A → B (or B → A, quote response) | Request a quote for a task; or respond with a quote | `agent_id_a`, `agent_id_b`, `agent_nonce`, `task_spec` (JSON), `quote?` (amount_sc, deadline_blocks) |
| `ACP_TASK` | A → B | Assign a task; references on-chain escrow txid | `agent_id_a`, `agent_id_b`, `agent_nonce`, `task_spec`, `escrow_txid`, `deadline_block` |
| `ACP_SETTLE` | A → B (or B → A, mutual) | Release escrow to B (task done) | `agent_id_a`, `agent_id_b`, `agent_nonce`, `escrow_txid`, `settle_signature_a`, `settle_signature_b?` (mutual) |
| `ACP_REVOKE` | A or B | Cancel task, refund escrow to A (after deadline or mutual) | `agent_id_a`, `agent_id_b`, `agent_nonce`, `escrow_txid`, `revoke_signature_a`, `revoke_signature_b?` (mutual) or `deadline_passed: bool` |

All messages are signed by the sender's agent keypair (secp256k1, per ADR-0028).
The signature covers the canonical serialization of all fields except the
signature itself.

### Wire format

ACP messages extend the `strangecoin-net::protocol::Message` enum (Stage 2)
with four new variants. Payload is canonical JSON (sorted keys, deterministic
byte order) wrapped in a signed envelope:

```
AcpEnvelope {
  message: AcpMessage,         // NEGOTIATE | TASK | SETTLE | REVOKE
  sender_agent_id: AgentId,    // secp256k1 pubkey (33 bytes compressed)
  signature: [u8; 64],         // secp256k1 over canonical_json(message || sender_agent_id)
}
```

### Transport

ACP messages ride the existing Stage 2 Noise-authenticated peer connection
(no new handshake). The agent runtime that owns the agent keypair is
co-located with the Strangecoin node (same process via SDK, or adjacent
process via JSON-RPC). The node signs the ACP envelope with the agent
keypair (via `AgentKeystore`, ADR-0028) and sends it over Noise to the peer
node, which forwards it to its co-located agent runtime.

Rate limit: ACP messages use a per-message-type sub-bucket of the Stage 2
token-bucket (ADR-0023). Defaults: `acp_negotiate_capacity = 100`,
`acp_negotiate_refill = 10/sec` (per peer). Mitigates V-44 (ACP spam).

### Escrow verification (invariant #24)

When `AcpHandler` receives `ACP_TASK` or `ACP_SETTLE` or `ACP_REVOKE`, it
validates `escrow_txid` via `StorageEngine::get_tx(txid)`:

1. **Existence.** txid must exist in chain (`transactions` CF, ADR-0024).
2. **Type.** tx must be a `call` to the `EscrowVault` contract (deployed in
   AI-P03).
3. **Parties.** tx sender = `agent_id_a`; tx args include `agent_b =
   agent_id_b`; amount ≥ quote from `ACP_NEGOTIATE`.
4. **Deadline.** tx args include `deadline_block`; current height ≤ deadline
   for `ACP_SETTLE`, or current height > deadline for `ACP_REVOKE` (unless
   mutual signatures).

On any check failure: reject the message, decrement peer score (Stage 2
scoring extension), and if repeated, ban the peer (ADR-0022 ladder).

### Replay protection (invariant #25)

Each ACP message carries `agent_nonce: u64`, monotonically increasing per
`(agent_id, counterparty)` pair. The `AcpHandler` maintains an in-memory
`HashMap<(AgentId, AgentId), u64>` of last-seen nonces. A message with
`agent_nonce <= last_seen` is rejected as a replay.

The nonce map is backed by RocksDB `meta` CF (key `acp_nonce:{agent_id_a}:{agent_id_b}`)
for restart-survival. On node restart, the map is loaded from `meta` CF.

### Settlement flow

```
Agent A                 Agent B
   │                       │
   ├─ ACP_NEGOTIATE ──────▶│  (A requests quote for task T)
   │                       │
   │◀── ACP_NEGOTIATE ─────┤  (B responds with quote: 50 SC, deadline=1000 blocks)
   │                       │
   │   (A deposits 50 SC to EscrowVault on-chain, gets escrow_txid)
   │                       │
   ├─ ACP_TASK ───────────▶│  (A assigns task, escrow_txid, deadline_block)
   │                       │
   │                       │   (B performs task)
   │                       │
   │◀── (off-chain task output, e.g., via IPFS / direct upload)
   │                       │
   ├─ ACP_SETTLE ─────────▶│  (A releases escrow to B, with settle_signatures)
   │                       │
   │   (AcpHandler triggers EscrowVault.release on-chain → 50 SC to B)
   │                       │
```

`ACP_REVOKE` flow (timeout or mutual cancel):

```
Agent A                 Agent B
   │                       │
   ├─ ACP_TASK ───────────▶│
   │                       │
   │   (B does not respond by deadline_block)
   │                       │
   ├─ ACP_REVOKE ─────────▶│  (A revokes, deadline_passed=true)
   │                       │
   │   (AcpHandler triggers EscrowVault.refund on-chain → 50 SC back to A)
```

### `EscrowVault` WASM contract (Stage 1.5)

Deployed at a fixed address (gen 1 contract, deterministic address). ABI:

- `deposit(agent_a: address, agent_b: address, amount: u64, deadline_block: u64) → escrow_id: u64`
  Called by agent A. Transfers `amount` SC from A to EscrowVault, records
  escrow. Returns `escrow_id`.
- `release(escrow_id: u64, signature_a: [u8;64], signature_b: [u8;64]) → bool`
  Called by AcpHandler on `ACP_SETTLE`. Requires both signatures (mutual).
  Transfers escrowed `amount` to agent B.
- `refund(escrow_id: u64, signature_a: [u8;64]) → bool` (if deadline passed) OR
  `refund(escrow_id: u64, signature_a: [u8;64], signature_b: [u8;64]) → bool` (mutual)
  Called by AcpHandler on `ACP_REVOKE`. Transfers escrowed `amount` back to
  agent A.

The contract emits events `EscrowDeposited(escrow_id, agent_a, agent_b, amount, deadline_block)`,
`EscrowReleased(escrow_id, agent_b)`, `EscrowRefunded(escrow_id, agent_a)` —
indexed by Stage 4 indexer for `agent_getActivity`.

### No on-chain ACP message storage

ACP messages are transport-layer only. The chain does not record
`ACP_NEGOTIATE` / `ACP_TASK` / `ACP_SETTLE` / `ACP_REVOKE` messages — only
the `EscrowVault.deposit` / `.release` / `.refund` txs (which are normal
WASM contract calls). The Stage 4 indexer indexes ACP messages off-chain
(by listening to the `NodeEvent` bus or by receiving ACP message copies
from the `AcpHandler`) for `agent_getActivity` queries.

### Crate structure

```
crates/strangecoin-net/src/
├── protocol/
│   ├── mod.rs         # Message enum extended with ACP_* variants
│   └── acp.rs         # NEW: AcpMessage enum, AcpNegotiate/AcpTask/AcpSettle/AcpRevoke structs
├── acp/               # NEW
│   ├── mod.rs
│   └── handler.rs     # AcpHandler — validates escrow txid (#24), checks nonce (#25), peer score updates
└── peer/
    └── scoring.rs     # Extended: ACP violations add penalty

crates/strangecoin-vm-wasm/examples/
└── escrow_vault.rs    # NEW: EscrowVault WASM contract source + ABI

tests/
└── acp_messages.rs    # NEW: e2e tests (negotiate→task→settle, revoke, fraud, replay, spam)

fuzz/fuzz_targets/
└── acp_handler.rs     # NEW: fuzz AcpHandler with random messages
```

## Consequences

### Positive

- **No central marketplace.** Agents negotiate peer-to-peer; no intermediary,
  no broker fees, no single point of failure.
- **Micropayments feasible.** Only 2 on-chain txs per task (deposit + settle/
  revoke); N off-chain ACP messages. Gas cost amortized across task.
- **Escrow publicly verifiable.** `EscrowVault` contract calls are on-chain;
  anyone can audit task payment history via indexer.
- **Stage 2 transport reuse.** No new crypto, no new network stack — Noise-XX
  + ChaCha20-Poly1305 + peer scoring + rate limit all reused.
- **Replay protection robust.** Monotonic nonces per counterparty, backed by
  RocksDB `meta` CF — survives restarts.

### Negative

- **`EscrowVault` WASM contract needs audit.** The contract holds escrowed
  funds; bugs are direct loss of agent funds. Mitigation: Stage 6 external
  audit covers this; Stage 4 ships unit tests + fuzz target.
- **ACP message loss → escrow lock.** If `ACP_SETTLE` is lost in transit and
  not retransmitted, escrow is locked until `deadline_block` passes, then A
  must `ACP_REVOKE` to refund. Mitigation: agent SDK (AI-P05) auto-retries
  `ACP_SETTLE` with exponential backoff; indexer (Stage 4) detects stuck
  escrows and alerts.
- **In-memory nonce map state loss without RocksDB backup.** If RocksDB
  write to `meta` CF fails (rare), nonce map could roll back, enabling
  replay. Mitigation: synchronous write to `meta` CF before processing
  message; fail-closed on write error.
- **`AcpHandler` complexity.** Validates escrow txid (3 checks), checks nonce,
  updates peer score, triggers EscrowVault release/refund. Many moving parts.
  Mitigation: fuzz target `acp_handler` runs 600s in CI.
- **ACP message indexing for `agent_getActivity`.** Stage 4 indexer must
  listen to ACP messages (either subscribe to `AcpHandler` event stream, or
  receive copies). Adds indexer complexity.

### Neutral

- **ACP is application-layer, not consensus.** Agents can choose not to use
  ACP and pay directly via on-chain txs (e.g., for non-escrowed services).
  ACP is opt-in per task.
- **HTLC-style off-chain settlement deferred.** Stage 4 uses on-chain
  `EscrowVault` for settlement (publicly verifiable). Off-chain HTLC (cheaper,
  indexer-trusted) is Stage 5+ research (ADR amendment or new ADR-0034).

## Alternatives

### Alternative 1: On-chain escrow (every ACP message is a tx)

- **Pros:** Maximally verifiable; no off-chain trust.
- **Cons:** Gas cost per message (NEGOTIATE, TASK, SETTLE, REVOKE = 4 txs per
  task) makes micropayments infeasible (gas > task value for sub-cent tasks).
- **Why not chosen:** Micropayment use case requires off-chain messages;
  on-chain only for deposit + settle/revoke (2 txs).

### Alternative 2: Off-chain HTLC-style settlement

- **Pros:** Cheaper than on-chain escrow (no deposit/settle gas); near-instant.
- **Cons:** Requires indexer trust for verification (HTLC proofs are
  off-chain); harder to audit.
- **Why not chosen:** Stage 4 prioritizes auditability (on-chain escrow).
  Off-chain HTLC is Stage 5+ research.

### Alternative 3: Centralized marketplace (broker)

- **Pros:** Simplest implementation (broker holds escrow, matches tasks).
- **Cons:** Trusted intermediary; fees; single point of failure; anti-goal
  (ARCHITECT.md §19.7 — no central marketplace).
- **Why not chosen:** Anti-goal. P2P is the whole point.

### Alternative 4: ACP over plain TCP (without Noise)

- **Pros:** Slightly simpler (no Noise-XX setup per ACP session).
- **Cons:** No encryption, no authentication, no replay protection at
  transport layer; anti-goal (§19.7).
- **Why not chosen:** Anti-goal. Noise is already deployed (Stage 2); reuse
  it.

## Related

- [ADR-0001](0001-secp256k1-vs-ed25519.md) — secp256k1 for EOA (agents use same curve; ACP signatures are secp256k1)
- [ADR-0020](0020-noise-protocol-framework.md) — Noise transport (reused for ACP-over-Noise)
- [ADR-0022](0022-peer-scoring-algorithm.md) — Peer scoring (extended for ACP violations)
- [ADR-0023](0023-rate-limiting-strategy.md) — Rate limiting (reused for ACP spam mitigation)
- [ADR-0024](0024-storage-schema-and-engine.md) — Storage schema (provides `StorageEngine::get_tx(txid)` for escrow verification #24; `meta` CF for nonce map #25)
- [ADR-0028](0028-strangecoin-as-cryptocurrency-for-ai-agents.md) — AI agent vision (this ADR operationalizes agent-to-agent)
- [ADR-0029](0029-mcp-server.md) — MCP server (complementary LM-facing facade)
- [ADR-0031](0031-professional-ui-replacement.md) — Professional UI (Agent Activity screen surfaces ACP messages)
- [ARCHITECT.md §19.3/§19.5/§19.6](../../analytics/ARCHITECT.md) — ACP subsystem, invariants #24/#25, threat vectors V-44/V-45
- [docs/spec/acp_messages.md](../spec/acp_messages.md) — ACP message spec (schemas, wire format, escrow flow diagrams, replay protection, rate limit, peer scoring extensions)
- [prompt-stage3.md §9 AI-P03](../../analytics/prompt-stage3.md) — Implementation prompt
- [BACKLOG.md](../../analytics/BACKLOG.md) — Work item tracking
