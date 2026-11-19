# Strangecoin ACP (Agent Communication Protocol) Messages Spec

**Status:** Implemented v1.1 (2026-11-19, `[S4-AI-P03]`) — original
Proposed v1.0 of 2026-11-04
**Implementation prompt:** `analytics/prompt-stage3.md` §9 `AI-P03`
**Authoritative ADR:** [ADR-0030](../ADR/0030-acp-agent-communication-protocol.md)
(superseded-in-part by its *Implementation amendments* section — deviations
are listed there, mirrored in §11.1 below)
**Related specs:** [agent_integration.md](agent_integration.md), [mcp_server.md](mcp_server.md)
**Invariants enforced:** #24 (ACP escrow txid real), #25 (ACP replay protection)
**Threat vectors mitigated:** V-44 (ACP spam), V-45 (ACP escrow fraud)
**Spec version:** 1.1

---

## 1. Overview

ACP (Agent Communication Protocol) is a wire message family that lets
Strangecoin AI agents communicate peer-to-peer — negotiate tasks, quote and
pay for services, and settle micropayments — without a centralized
marketplace. ACP messages ride the existing Stage 2 Noise-authenticated P2P
transport (ADR-0020); they are **application-layer** messages, not a new
transport.

**Four message types:**

| Message | Purpose |
|---------|---------|
| `ACP_NEGOTIATE` | Request a quote for a task, or respond with a quote |
| `ACP_TASK` | Assign a task; references on-chain escrow txid |
| `ACP_SETTLE` | Release escrow to the task performer (task done) |
| `ACP_REVOKE` | Cancel task, refund escrow to the assigner (after deadline or mutual cancel) |

**Two on-chain transactions per task:**

1. `EscrowVault.deposit(agent_a, agent_b, amount, deadline_block)` — agent A
   deposits escrow when assigning task (`ACP_TASK`).
2. `EscrowVault.release(escrow_id, sig_a, sig_b)` (on `ACP_SETTLE`) or
   `EscrowVault.refund(escrow_id, sig_a)` (on `ACP_REVOKE`).

All other messages (NEGOTIATE, the messages themselves) are off-chain,
transported over Noise P2P.

---

## 2. Wire format

ACP messages extend the `strangecoin-net::protocol::Message` enum (Stage 2)
with four new variants. Payload is canonical JSON wrapped in a signed
envelope:

### 2.1. AcpEnvelope

```rust
struct AcpEnvelope {
    message: AcpMessage,        // NEGOTIATE | TASK | SETTLE | REVOKE
    sender_agent_id: AgentId,   // secp256k1 pubkey (33 bytes compressed, hex)
    signature: [u8; 64],        // secp256k1 over canonical_json(message || sender_agent_id)
}
```

- **Canonical JSON:** sorted keys, deterministic byte order, no whitespace.
- **Signature:** secp256k1 (same curve as EOA, ADR-0001) over the
  canonical JSON of `(message, sender_agent_id)`.
- **AgentId:** 33-byte compressed secp256k1 pubkey (hex-encoded in JSON,
  66 chars).

### 2.2. AcpMessage (enum)

```rust
enum AcpMessage {
    Negotiate(AcpNegotiate),
    Task(AcpTask),
    Settle(AcpSettle),
    Revoke(AcpRevoke),
}
```

---

## 3. Message schemas

### 3.1. `ACP_NEGOTIATE`

Used in two directions:

- **Request quote:** A → B, "what's your quote for task T?"
- **Respond with quote:** B → A, "my quote is X SC, deadline D blocks."

```json
{
  "type": "ACP_NEGOTIATE",
  "agent_id_a": "0x...33_bytes...",
  "agent_id_b": "0x...33_bytes...",
  "agent_nonce": 42,
  "task_spec": {
    "description": "summarize document X",
    "input_ref": "ipfs://Qm...",
    "output_format": "markdown",
    "max_length": 5000
  },
  "quote": {
    "amount_sc": "50",
    "deadline_blocks": 1000
  }
}
```

- `agent_nonce` — monotonic per `(agent_id_a, agent_id_b)` pair (replay
  protection, invariant #25).
- `task_spec` — JSON describing the task. Schema is task-specific (no
  canonical schema enforced by ACP; agents interpret it bilaterally).
- `quote` — present only in B → A direction (quote response). Absent in
  A → B direction (quote request).

### 3.2. `ACP_TASK`

Sent by A → B after `ACP_NEGOTIATE` succeeds and A has deposited escrow
on-chain.

```json
{
  "type": "ACP_TASK",
  "agent_id_a": "0x...",
  "agent_id_b": "0x...",
  "agent_nonce": 43,
  "task_spec": { ... },
  "escrow_txid": "0x...64_hex_chars...",
  "deadline_block": 5000
}
```

- `escrow_txid` — txid of the on-chain `EscrowVault.deposit(agent_a,
  agent_b, amount, deadline_block)` tx. Must exist in chain (invariant
  #24, verified by `AcpHandler`).
- `deadline_block` — block height by which B must complete the task.
  After this, A can `ACP_REVOKE` unilaterally.

### 3.3. `ACP_SETTLE`

Sent by A → B when A acknowledges task completion and releases escrow to B.

```json
{
  "type": "ACP_SETTLE",
  "agent_id_a": "0x...",
  "agent_id_b": "0x...",
  "agent_nonce": 44,
  "escrow_txid": "0x...",
  "settle_signature_a": "0x...64_bytes_secp256k1_over_escrow_id...",
  "settle_signature_b": "0x...64_bytes_secp256k1_over_escrow_id..."
}
```

- Both signatures required (mutual consent). `settle_signature_a` is over
  `escrow_id` (derived from `escrow_txid`); `settle_signature_b` is the
  same.
- On receipt, `AcpHandler` triggers `EscrowVault.release(escrow_id, sig_a,
  sig_b)` on-chain → escrowed amount transferred to B.

### 3.4. `ACP_REVOKE`

Sent by A or B to cancel the task and refund escrow to A.

```json
{
  "type": "ACP_REVOKE",
  "agent_id_a": "0x...",
  "agent_id_b": "0x...",
  "agent_nonce": 45,
  "escrow_txid": "0x...",
  "revoke_signature_a": "0x...",
  "revoke_signature_b": "0x...",
  "deadline_passed": false
}
```

Two cases:

- **Mutual cancel (before deadline):** both signatures required,
  `deadline_passed = false`.
- **Unilateral (after deadline):** only `revoke_signature_a` required,
  `deadline_passed = true`. `AcpHandler` verifies current block height >
  `deadline_block` from the original `EscrowVault.deposit` tx.

On receipt, `AcpHandler` triggers `EscrowVault.refund(escrow_id, sig_a)` or
`EscrowVault.refund(escrow_id, sig_a, sig_b)` on-chain → escrowed amount
transferred back to A.

---

## 4. Escrow flow

### 4.1. Happy path (negotiate → task → settle)

```
Agent A                                     Agent B
   │                                           │
   1. ACP_NEGOTIATE (quote request)           │
   │──────────────────────────────────────────▶│
   │                                           │
   │                  2. ACP_NEGOTIATE (quote response: 50 SC, deadline=1000 blocks)
   │◀──────────────────────────────────────────│
   │                                           │
   3. (A deposits 50 SC to EscrowVault on-chain, gets escrow_txid)
   │                                           │
   4. ACP_TASK (escrow_txid, deadline_block)   │
   │──────────────────────────────────────────▶│
   │                                           │
   │                                           │   5. (B performs task, sends output via
   │                                           │      off-chain channel — IPFS / direct upload)
   │                                           │
   6. ACP_SETTLE (sig_a, sig_b)                │
   │──────────────────────────────────────────▶│
   │                                           │
   7. (AcpHandler triggers EscrowVault.release → 50 SC to B)
   │                                           │
```

### 4.2. Timeout / no response (revoke after deadline)

```
Agent A                                     Agent B
   │                                           │
   1-4. (negotiate, task as above)             │
   │                                           │
   │                                           │   5. (B does not respond; deadline_block passes)
   │                                           │
   6. ACP_REVOKE (deadline_passed=true, sig_a) │
   │──────────────────────────────────────────▶│
   │                                           │
   7. (AcpHandler verifies current_height > deadline_block, triggers
      EscrowVault.refund → 50 SC back to A)
   │                                           │
```

### 4.3. Mutual cancel (revoke before deadline)

```
Agent A                                     Agent B
   │                                           │
   1-4. (negotiate, task as above)             │
   │                                           │
   │                                           │   5. (B realizes cannot complete; sends
   │                                           │      ACP_REVOKE with sig_b)
   6. ACP_REVOKE (deadline_passed=false,       │
   │   sig_a, sig_b)                           │
   │◀──────────────────────────────────────────│   (or A→B if A initiates)
   │                                           │
   7. (AcpHandler triggers EscrowVault.refund(escrow_id, sig_a, sig_b)
      → 50 SC back to A)
   │                                           │
```

---

## 5. Escrow verification (invariant #24)

When `AcpHandler` receives `ACP_TASK`, `ACP_SETTLE`, or `ACP_REVOKE`, it
validates `escrow_txid` via `StorageEngine::get_tx(txid)`:

| Check | What it verifies | Failure action |
|-------|------------------|----------------|
| **Existence** | txid exists in chain (`transactions` CF, ADR-0024) | Reject message, peer score -0.1 |
| **Type** | tx is a `call` to `EscrowVault` contract address | Reject, peer score -0.2 |
| **Parties** | tx sender = `agent_id_a`; tx args include `agent_b = agent_id_b` | Reject, peer score -0.3 (likely fraud, V-45) |
| **Amount** | tx args `amount ≥ quote.amount` from prior `ACP_NEGOTIATE` | Reject, peer score -0.3 |
| **Deadline** | For `ACP_REVOKE` with `deadline_passed=true`: current height > tx args `deadline_block` | Reject, peer score -0.1 (premature revoke) |
| **Settle signatures** | For `ACP_SETTLE`: `sig_a` and `sig_b` verify against `agent_id_a` and `agent_id_b` over `escrow_id` | Reject, peer score -0.3 (forged signature) |

If peer score drops below `peer_ban_threshold` (0.2, ADR-0022), peer is
temporarily banned (1h). Below `peer_scoring_persistent_threshold` (0.05),
permanently banned (30d). Repeated ACP fraud → permanent ban.

---

## 6. Replay protection (invariant #25)

Each ACP message carries `agent_nonce: u64`, monotonically increasing per
`(agent_id_a, agent_id_b)` pair.

### 6.1. Nonce tracking

`AcpHandler` maintains an in-memory map:

```rust
HashMap<(AgentId, AgentId), u64>  // (sender, receiver) → last_seen_nonce
```

On incoming message:

1. Look up `last_seen_nonce` for `(sender_agent_id, receiver_agent_id)`.
2. If `message.agent_nonce <= last_seen_nonce` → reject (replay).
3. Else, update `last_seen_nonce = message.agent_nonce` and process.

### 6.2. Persistence

The nonce map is backed by RocksDB `meta` CF (ADR-0024):

- Key: `acp_nonce:{agent_id_a_hex}:{agent_id_b_hex}`
- Value: `u64` last-seen nonce (big-endian, 8 bytes).

On node restart, the in-memory map is loaded from `meta` CF. Writes to
`meta` CF are **synchronous** (write-ahead before processing the message)
— if the write fails, the message is rejected (fail-closed) to prevent
state loss enabling replay.

### 6.3. Bidirectional nonces

Nonces are tracked per direction: `(A, B)` and `(B, A)` are separate
counters. This allows both agents to send messages without coordinating
nonce allocation.

---

## 7. Rate limit (V-44 mitigation)

ACP messages use per-message-type sub-buckets of the Stage 2 token-bucket
(ADR-0023):

```toml
[rate_limit.acp]
negotiate_capacity = 100       # max ACP_NEGOTIATE in flight per peer
negotiate_refill = 10          # tokens/sec
task_capacity = 20             # max ACP_TASK in flight per peer
task_refill = 2                # tokens/sec
settle_capacity = 20
settle_refill = 2
revoke_capacity = 10
revoke_refill = 1
```

On bucket exhaustion: message dropped, peer score -0.05 per drop. Repeated
exhaustion → temp ban.

---

## 8. EscrowVault WASM contract

Deployed at a fixed address (deterministic deployment via `create2`-style
nonce — Stage 1.5 contract deployment). ABI:

### 8.1. `deposit`

```
deposit(agent_a: address, agent_b: address, amount: u64, deadline_block: u64) → escrow_id: u64
```

- **Caller:** agent A (signs the `call` tx).
- **Action:** transfers `amount` SC from `agent_a` to EscrowVault contract
  account; records escrow `(escrow_id, agent_a, agent_b, amount,
  deadline_block, status=pending)`.
- **Returns:** `escrow_id` (monotonic counter).
- **Events:** `EscrowDeposited(escrow_id, agent_a, agent_b, amount,
  deadline_block)`.

### 8.2. `release`

```
release(escrow_id: u64, signature_a: [u8;64], signature_b: [u8;64]) → bool
```

- **Caller:** `AcpHandler` (via `agent_signAndSubmitBatch` or direct
  `call`).
- **Action:** verifies both signatures over `escrow_id` (sig_a from
  agent_a, sig_b from agent_b); transfers `amount` SC from EscrowVault to
  `agent_b`; marks escrow `status=settled`.
- **Returns:** `true` on success, reverts on signature failure or
  already-settled escrow.
- **Events:** `EscrowReleased(escrow_id, agent_b)`.

### 8.3. `refund`

```
refund(escrow_id: u64, signature_a: [u8;64]) → bool   // deadline passed

refund(escrow_id: u64, signature_a: [u8;64], signature_b: [u8;64]) → bool   // mutual
```

- **Caller:** `AcpHandler`.
- **Action (deadline_passed case):** verifies `signature_a`; checks
  `current_block_height > deadline_block`; transfers `amount` SC from
  EscrowVault to `agent_a`; marks escrow `status=refunded_timeout`.
- **Action (mutual case):** verifies both signatures; transfers `amount`
  to `agent_a`; marks escrow `status=refunded_mutual`.
- **Returns:** `true` on success, reverts on sig failure, premature
  deadline, or already-refunded escrow.
- **Events:** `EscrowRefunded(escrow_id, agent_a, reason)` where reason ∈
  `{timeout, mutual}`.

---

## 9. Peer scoring extensions

Stage 2 peer scoring (ADR-0022) is extended for ACP violations:

| Violation | Score delta | Notes |
|-----------|-------------|-------|
| Invalid escrow_txid (non-existent or wrong type) | -0.1 | Likely a bug; warning |
| Mismatched agent_b in escrow | -0.3 | Likely fraud (V-45) |
| Forged settle_signature | -0.3 | Definitely fraud |
| Premature revoke (deadline_passed=true but deadline not passed) | -0.1 | Bug or attack |
| Rate limit exhaustion (V-44) | -0.05 per drop | Spam pattern |
| Valid message | +0.01 | Trust building |

Cumulative score drops trigger the existing Stage 2 ladder (warn →
temp-ban → persistent-ban, ADR-0022).

---

## 10. Crate structure

```
crates/strangecoin-net/src/
├── protocol/
│   ├── mod.rs                # Message enum extended with ACP_* variants
│   └── acp.rs                # NEW: AcpMessage enum, AcpNegotiate/Task/Settle/Revoke structs
├── acp/                      # NEW
│   ├── mod.rs
│   ├── handler.rs            # AcpHandler — validates escrow txid, checks nonce, peer score
│   └── nonce.rs              # Nonce map (in-memory + RocksDB meta CF backed)
└── peer/
    └── scoring.rs            # Extended: ACP violations add penalty

crates/strangecoin-vm-wasm/examples/
└── escrow_vault.rs           # NEW: EscrowVault WASM contract source + ABI

tests/
└── acp_messages.rs           # NEW: e2e tests

fuzz/fuzz_targets/
└── acp_handler.rs            # NEW: fuzz AcpHandler with random messages
```

---

## 11. Test plan

### 11.1. Status (all green, `[S4-AI-P03]`, 2026-11-19)

`tests/acp_messages.rs` (root integration suite) — 8/8:

| Test | What it verifies | Status |
|------|------------------|--------|
| `acp_negotiate_task_settle_e2e` | Two-agent session: A negotiates, B quotes, A assigns + escrows, B fulfills, A settles → on-chain release to B (real `EscrowVault.release` tx mined; B's balance +50, vault → 0) | ✅ |
| `acp_revoke_after_timeout` | A assigns, B no response, chain walked past `deadline_block` (crafted blocks, no wall time), A revokes → on-chain refund to A | ✅ |
| `acp_revoke_mutual_cancel` | Mutual cancel before deadline (both sigs, `deadline_passed = false`) → refund to A | ✅ |
| `acp_escrow_fraud_rejected` | SETTLE with a never-mined txid → `MissingEscrowTx` rejected + peer banned (−0.1 = 2 violation units under tuned weights; V-45) | ✅ |
| `acp_replay_rejected` | Byte-identical redelivery with the same `agent_nonce` → `Replay`, nonce window monotonic, peer charged one protocol violation (#25) | ✅ |
| `acp_spam_rate_limited` | 300 `ACP_NEGOTIATE` at one injected instant → exactly the spec §7 burst (100) admitted through the Stage 2 sub-bucket, the rest refused, `apply_rate_verdict` escalates to a peer ban (V-44) | ✅ |
| `acp_premature_revoke_rejected` | `deadline_passed = true` before the deadline → `EscrowDeadline` (−0.1), funds untouched | ✅ |
| `acp_forged_settle_signature_rejected` | SETTLE over the real escrow with a forged agent-B signature → `ForgedEscrowSignature` (−0.3) + ban (V-45) | ✅ |

Contract-level matrix — `cargo test -p strangecoin-vm-wasm --example
escrow_vault` (8/8): deposit record/event/value-binding, value-mismatch
revert (drain guard), duplicate-calldata revert, release pays B once
(double-release traps), forged settle sigs, timeout refund before/after
deadline + double-refund trap, mutual cancel + forged mutual sig.

Wire/handler units — `cargo test -p strangecoin-net --features acp`
(86 lib tests incl. codec round-trips, canonical signing bytes, envelope
tamper/tag-mismatch/oversize, nonce-store round-trip) plus
`tests/acp_messages.rs` (8) and `tests/determinism`-style cross-checks.

CI gates:

- `cargo test -p strangecoin-net --features acp` (all tests green)
- `cargo clippy -p strangecoin-net --features acp --all-targets -- -D warnings`
  (green; the literal bare `cargo clippy -p strangecoin-net -- -D warnings`
  from the prompt also green)
- `cargo +nightly fuzz run acp_handler -- -max_total_time=600` — target
  added to the nightly matrix (11th); local verification via the S2-P21
  stub-harness fallback (see `fuzz/README.md`), coverage-guided run is
  CI-only on this host (FBP08/FBP11 precedent)

### 11.2. Implementation notes (v1.1 deltas vs the v1.0 text above)

1. **§2/§3 wire**: exactly as specified; `quote.amount_sc` is a JSON number
   (u64), not the §3.1 example's string; envelopes capped at 64 KiB.
2. **§3.2 deadline binding**: `ACP_TASK.deadline_block` must equal the
   on-chain deposit `deadline_block` — mismatch rejects as `EscrowDeadline`
   (hardening beyond v1.0).
3. **§5 checks**: implemented in `strangecoin-net/src/acp/handler.rs` in
   the documented order (existence → type → parties → amount → deadline →
   signatures). Amount-vs-quote runs only when this node recorded a quote
   (session-scoped memory; the counterparty node never saw the B → A
   response); `tx.value ≥ amount` and the pubkey/address party checks are
   unconditional. The contract independently enforces
   `record.amount == sc.value()` (new `sc.value` host function — ADR-0030
   amendment 1).
4. **§6 nonce**: as specified, write-ahead + fail-closed; production store
   is `src/storage/acp_nonce.rs` (`meta` CF, lazy dual-read), tests use the
   in-memory `MemoryNonceStore`.
5. **§7 rate limit**: sub-buckets wired as `rate_limit_per_type` keys
   `acp_negotiate` / `acp_task` / `acp_settle` / `acp_revoke` (spec §7
   defaults) mapped onto `MessageType::Acp*`.
6. **§8 ABI**: fixed flat calldata layout (addresses length-prefixed +
   compressed pubkeys; release/refund `id ‖ sig_a ‖ [sig_b]`, mode by 76 vs
   140 byte length) — codec `strangecoin_core::escrow`, single source for
   handler, contract and tests. `escrow_id = blake3(deposit_calldata)[0..8]`
   (ADR-0030 amendment 3). Events: `topic0 = [tag, 0*31]` with
   tag ∈ {1 deposited, 2 released, 3 refunded}; data carries
   `id ‖ amount ‖ deadline` (deposit) or `id` (release/refund).
7. **§9 scoring**: escrow rows map to violation units (−0.1 → 2, −0.2 → 4,
   −0.3 → 6 at default weights; `scoring::AcpViolation`). Replay /
   bad-signature / non-party ride the generic protocol channel (+1); the
   "valid +0.01" row is not modeled (ADR-0022 has no positive channel).
   Rate-limit drops already cost exactly −0.05 per refusal through
   `record_rate_violation`.
8. **§8 caller**: production `EscrowSettlement` = Noop until AI-P04
   session keys; the e2e suite injects a chain-backed adapter (ADR-0030
   amendment 2).
9. **§10 crate structure**: as built, plus `strangecoin_core::escrow`
   (shared codec) and `src/storage/acp_nonce.rs` (monolith nonce store);
   the contract example doubles as the WAT source pulled into the e2e via
   `#[path]`.

## 12. Anti-goals

- **ACP does not store messages on-chain.** ACP messages are transport-
  layer only; the chain records only `EscrowVault.deposit`/`.release`/
  `.refund` txs.
- **ACP does not relay via third peer.** Stage 4 — direct peer-to-peer
  only (agents must be connected directly). Relayed ACP (gossip-style) is
  Stage 5+ research.
- **ACP does not do off-chain HTLC settlement.** Stage 4 uses on-chain
  `EscrowVault` for settlement (publicly verifiable). Off-chain HTLC is
  Stage 5+ research.
- **ACP does not implement ZK-proofs for escrow.** Stage 4 verifies escrow
  txid on-chain only. ZK-ACP is Stage 6+ research.

---

## 13. References

- [ADR-0030](../ADR/0030-acp-agent-communication-protocol.md)
- [ADR-0020](../ADR/0020-noise-protocol-framework.md) — Noise transport (reused)
- [ADR-0022](../ADR/0022-peer-scoring-algorithm.md) — Peer scoring (extended)
- [ADR-0023](../ADR/0023-rate-limiting-strategy.md) — Rate limiting (reused)
- [ADR-0024](../ADR/0024-storage-schema-and-engine.md) — Storage (provides `get_tx` for escrow verification, `meta` CF for nonce map)
- [ADR-0028](../ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md) — Agent vision
- [agent_integration.md](agent_integration.md) — Agent identity
- [ARCHITECT.md §19.3/§19.5/§19.6](../../analytics/ARCHITECT.md)
- [prompt-stage3.md §9 AI-P03](../../analytics/prompt-stage3.md)
