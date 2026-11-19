# ADR-0030: ACP (Agent Communication Protocol)

## Status

Accepted (2026-11-19) — `AI-P03` (extension prompt in `analytics/prompt-stage3.md` §9).
Acceptance gate: land of ACP wire messages (`ACP_NEGOTIATE` / `ACP_TASK` / `ACP_SETTLE` / `ACP_REVOKE`),
`AcpHandler`, `EscrowVault` WASM contract, and `tests/acp_messages.rs`
end-to-end suite — done in the `[S4-AI-P03]` commit (see *Implementation
amendments* below for the deviations recorded at landing).

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

## Implementation amendments (AI-P03 landing, 2026-11-19)

Deviations from the literal text above, recorded per the FBP "deviations are
explicit" rule:

1. **`sc.value` host function added** (ADR-0014-class gas weight 3,
   `strangecoin-vm-wasm/src/host/context/value.rs`). The design assumed the
   contract could enforce "deposit records what it moved"; the VM exposes no
   value/caller view, and calldata alone is attackable — a deposit claiming
   1_000_000 SC while transferring 1 SC would later drain co-mingled
   escrows through `release`. The contract therefore binds
   `record.amount := sc.value()` and reverts unless the calldata amount
   equals it (host additive, pre-mainnet; existing contracts unaffected —
   only modules importing `sc.value` need it).
2. **Settlement is trait-injected; production wires `NoopEscrowVault` until
   AI-P04.** §"Settlement flow" has the handler "trigger" the on-chain call,
   but a settlement tx needs an agent signing key — which lands with
   AI-P04 `agent_signAndSubmitBatch` (session keys). Signing with the
   operator wallet would violate invariant #26, so `AcpHandler` holds an
   `EscrowSettlement` adapter: tests inject a chain-backed adapter (real
   `release`/`refund` txs, mined — the e2e proves the on-chain path), the
   node wires the Noop adapter (valid SETTLE/REVOKE pass validation, then
   surface locally as a non-scored `Settlement` reject until AI-P04).
3. **`escrow_id = blake3(deposit_calldata)[0..8]`**, not the §8 monotonic
   counter and not literally "derived from `escrow_txid`" (§3.3): the
   handler cannot know the contract's counter from the txid, and the
   contract cannot see the txid. Both sides recompute the id from identical
   bytes (handler: `tx.data`; contract: its own input). Signatures cover
   `blake3(id_bytes)` (32 B — the secp256k1 host takes a digest). A repeated
   *identical* deposit maps to an occupied storage key and reverts (the
   duplicate guard replacing counter uniqueness).
4. **Deposit calldata carries both bech32 addresses *and* both compressed
   pubkeys** (§8.1 ABI extended): the contract verifies settle/refund
   signatures (host needs 33-byte pubkeys — no bech32 decode exists) and
   pays out to bech32 addresses (no address-derivation host exists). Fixed
   flat layout with length-prefixed addresses; codec shared through
   `strangecoin_core::escrow`.
5. **Self-dispatch inside the `main` export** — the monolith still never
   registers `AbiDocument`s (ADR-0029 amendment), so the VM's selector
   dispatch always falls back to `main`; the contract compares the first
   four calldata bytes itself. Selectors remain `method_selector` blake3
   prefixes, so a future ABI registry stays compatible.
6. **Amount-vs-quote check is conditional on a recorded quote.** The
   counterparty's node never receives the B → A quote response (§5 «Amount»
   assumes the checker saw it), and quotes are session-scoped memory (no
   meta-CF persistence). Unconditional checks remain: parties (sender,
   calldata addresses, calldata pubkeys) and `tx.value ≥ amount`.
7. **Settlement acceptance rule tightened**: handler requires
   `tx.value ≥ recorded amount`, contract requires equality (amendment 1);
   `ACP_TASK.deadline_block` must equal the on-chain deposit deadline
   (mismatched deadlines would desync revoke rights).
8. **Scoring channels**: escrow failures use the spec §5/§9 table mapped to
   ADR-0022 violation units at default weights (−0.1 → 2, −0.2 → 4, −0.3 →
   6; `peer/scoring.rs::AcpViolation`). Replays, bad envelope signatures,
   non-party senders and malformed frames have no ACP row in §9 — they ride
   the generic protocol-violation channel (+1 unit each). The §9
   "valid message +0.01" row is **not modeled**: ADR-0022 has no
   positive-score channel.
9. **Feature `acp` in `strangecoin-net` is default-off**; the root crate's
   `net` feature enables it (`net = [..., "strangecoin-net/acp"]`), so every
   production node speaks ACP while a bare `-p strangecoin-net` build
   compiles both feature states (the КГ `--features acp` stays meaningful).
   A feature-off build never registers the tags — an ACP peer's frames
   decode as `UnknownMessage` and are log+ignored (S2-P18 forward-compat).
10. **`ConnectionContext.acp: Option<AcpHandler>`** (cfg'd field; the root
    call sites are unconditional since the root always enables `acp`), no
    `Capability::Acp` HELLO bit — the ADR never specified one; capability
    gating can arrive as a later amendment if cross-version negotiation
    becomes necessary.
11. **Nonce persistence = lazy dual-read** `MetaNonceStore` (memory map with
    `meta` CF fall-through, write-ahead on store, fail-closed): equivalent to
    §"on restart the map is loaded" without a bulk-load step. Key:
    `meta_key("acp_nonce:{sender_hex}:{counterparty_hex}")`, value BE `u64`.
12. **`ACP_NEGOTIATE.quote.amount_sc` is a JSON number** (u64) — §3.1's
    example showed a quoted string. ACP envelopes are capped at **64 KiB**
    (`MAX_ACP_ENVELOPE`) ahead of the 32 MiB frame cap.
13. **EscrowVault source lives at
    `crates/strangecoin-vm-wasm/examples/escrow_vault.rs`** (WAT assembled at
    runtime from the shared selectors); `tests/acp_messages.rs` pulls it in
    via `#[path]` so the contract has exactly one home, and the example's
    own unit matrix runs under `cargo test -p strangecoin-vm-wasm --example
    escrow_vault`.
14. **No on-chain ACP storage and no relay** (§"No on-chain ACP message
    storage", spec §12) unchanged: ACP frames are transport-layer only; the
    chain carries exactly the `EscrowVault` deposit/release/refund txs.

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
