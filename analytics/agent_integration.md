# Strangecoin AI Agent Integration Spec

**Status:** Proposed (2026-11-04)
**Implementation prompt:** `analytics/prompt-stage3.md` §9 `AI-P01`
**Authoritative ADR:** [ADR-0028](../ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md)
**Related ADRs:** [ADR-0029](../ADR/0029-mcp-server.md) (MCP), [ADR-0030](../ADR/0030-acp-agent-communication-protocol.md) (ACP), [ADR-0031](../ADR/0031-professional-ui-replacement.md) (Professional UI)
**Invariants enforced:** #26 (Agent key isolation)
**Threat vectors mitigated:** V-46 (Agent key theft via MCP stdio pipe)
**Spec version:** 1.0

---

## 1. Overview

Strangecoin is positioned as a **cryptocurrency for AI agents** (ADR-0028).
This spec defines the integration boundary between autonomous AI-agent
runtimes and the Strangecoin node. It covers agent identity, key management,
transaction patterns, and the off-chain agent registry.

Three complementary surfaces serve agent consumers:

1. **MCP server** (`docs/spec/mcp_server.md`, ADR-0029) — LLM-facing facade
   for tool-calling consumers (Claude, GPT, Gemini, open-weight via
   `mcp-cli`).
2. **ACP transport** (`docs/spec/acp_messages.md`, ADR-0030) — agent-to-agent
   wire protocol for task delegation, micropayments, escrow.
3. **Agent JSON-RPC namespace** (`agent_*` methods, Stage 4) — programmatic
   facade for code-first agent runtimes (LangChain, AutoGen, CrewAI,
   Rust-native, TypeScript-native).

This spec is high-level: per-surface details live in `mcp_server.md` and
`acp_messages.md`. Cross-cutting concerns (identity, keys, batched/sponsored
tx) live here.

---

## 2. Agent identity

### 2.1. Definition

An **AI agent** is a secp256k1 EOA (Externally Owned Account) owned by an
agent runtime (a software process that signs transactions autonomously). The
consensus layer treats agents identically to human wallets — agent-ness is a
property of how the key is used, not what the chain records.

- **No new account type.** Agents use the same `secp256k1` curve (ADR-0001)
  and the same bech32 address format as human wallets.
- **No on-chain agent registry.** Anti-goal (ARCHITECT.md §19.7). Agents are
  anonymous EOAs; reputation is an off-chain concern.
- **MetaMask/Ledger compatibility preserved.** An agent address can be
  imported into MetaMask (though the agent runtime, not MetaMask, signs).

### 2.2. Address format

Same as human wallets: bech32 with HRP `rsc1` (regtest), `tsc1` (testnet),
`sc1` (mainnet). Address derivation from secp256k1 pubkey is unchanged from
Stage 0.

### 2.3. HD derivation (BIP-44)

Agents MAY derive sub-keys and session keys via BIP-44 hierarchical
deterministic derivation:

```
Path: m / 44' / 1234' / <agent_id>' / <change> / <index>

  - purpose = 44' (BIP-44)
  - coin_type = 1234' (Strangecoin placeholder; SLIP-44 registration
    pending — AI-P05 opens the PR to satoshilabs/slips#slip-44)
  - agent_id = u32 (the agent's master account index, assigned by the
    agent runtime's key management)
  - change = 0 (external) / 1 (internal — used for session keys)
  - index = u32 (sub-agent index or session index)
```

**Examples:**

- `m/44'/1234'/0'/0/0` — first agent's first external address (default
  agent wallet)
- `m/44'/1234'/0'/0/1` — first agent's second external address (e.g., for
  a specific task counterparty)
- `m/44'/1234'/0'/1/0` — first agent's first session key (short-TTL,
  scoped — see §3)

### 2.4. SLIP-44 registration

Stage 4 deliverable (`AI-P05`): open a PR to
[satoshilabs/slips#slip-44](https://github.com/satoshilabs/slips) to register
`coin_type = 1234` as "Strangecoin". If accepted, the ADR-0028 is amended
with the registration link. If rejected, the placeholder is documented with
an explicit note that `1234` is unregistered but used consistently within
the Strangecoin ecosystem.

---

## 3. Key management

### 3.1. Key isolation (invariant #26)

The Strangecoin wallet module exposes two non-overlapping keystore types:

```rust
// In crates/strangecoin-core/src/wallet/keystore.rs (or src/wallet.rs)

pub trait OperatorKeystore {
    /// Returns the node operator's keypair — used for block production
    /// (PoW coinbase), fee collection, and (Stage 7) consensus message
    /// signing.
    fn operator_keypair(&self) -> &Keypair;
}

pub trait AgentKeystore {
    /// Returns an agent keypair by HD path. Available to MCP handlers,
    /// ACP handlers, and agent JSON-RPC methods.
    fn agent_keypair(&self, path: &DerivationPath) -> Result<Keypair>;
}
```

**Compile-time enforcement:** the two traits are separate types; there is no
shared trait object that exposes both. MCP handlers (`crates/strangecoin-mcp`)
and ACP handlers (`crates/strangecoin-net::acp`) take only `AgentKeystore` as
a dependency — they cannot access `OperatorKeystore` even via unsafe casts
(enforced by `cargo clippy` lint or type-system review).

### 3.2. Session keys (optional pattern)

An agent MAY derive a short-TTL session keypair for scoped operations:

```
Session keypair:
  - HD path: m/44'/1234'/{agent_id}'/1/{session_id}
  - TTL: configurable, default 1 hour (`agent.session_ttl_sec`)
  - Scope: signed by master agent key, scoped to specific contract
    addresses + method selectors + max value
```

The session key is validated by an on-chain WASM **policy wallet** contract
(Stage 1.5 WASM contract, deployed in Stage 4). The policy wallet checks
the master signature, scope, and TTL before accepting the session key's
signature.

**Use case:** an agent runtime wants to delegate limited authority to a
sandboxed tool runner (e.g., an LLM-tool that executes WASM contracts). The
session key limits blast radius — even if compromised, the attacker can only
call scoped methods up to the max value, until TTL expires.

### 3.3. Keystore file format

Operator and agent keys are stored in the same AES-256-GCM + PBKDF2 keystore
format (Stage 0) but in separate files:

- `<data_dir>/keystore/operator.json` — operator keypair (encrypted)
- `<data_dir>/keystore/agents/{agent_id}.json` — per-agent master keypair
  (encrypted)
- `<data_dir>/keystore/agents/{agent_id}/sessions/{session_id}.json` —
  session keypair (encrypted, short-TTL)

---

## 4. Transaction patterns

### 4.1. Standard transfer

Same as human wallets: a `transfer` tx signed by the agent's EOA keypair.
Tx type, gas, nonce — all identical to Stage 0/1 baseline.

### 4.2. Batched transactions (Stage 4)

One signature, N contract calls — implemented as a WASM contract
`BatchExecutor`:

```
BatchExecutor.execute(calldata_batch: bytes, signature: [u8;64]) → [result; N]

  calldata_batch = canonical serialization of N (target_address, method_selector, args) tuples
  signature = secp256k1 over calldata_batch (signed by agent EOA)

  For each (target, method, args):
    result[i] = CALL(target, method, args)  // reverts on failure
  Return [result; N]
```

The batch tx appears on-chain as a single `call` to `BatchExecutor`'s
address. Gas is the sum of N internal calls (plus batch overhead). Nonces:
one on-chain nonce consumed (the batch tx's nonce).

Surfaced in JSON-RPC as `agent_signAndSubmitBatch` (see `agent_*` spec
below) and in MCP as a helper that wraps `submit_tx` for the batch contract.

### 4.3. Sponsored transactions (Stage 4 stub, Stage 5 full)

A paymaster contract pays gas for an agent tx. Pattern:

```
1. Agent signs intended action (e.g., "transfer 100 SC to B") — signature_agent
2. Paymaster signs sponsorship ("I'll pay gas for tx X") — signature_paymaster
3. Sponsor tx submitted: paymaster is the gas-payer, agent's action is the
   payload, both signatures verified by Paymaster contract
```

**Stage 4:** `agent_sponsorTx` JSON-RPC method is a stub returning
`{"code": -32601, "message": "agent_sponsorTx not implemented until Stage 5
paymaster"}`.

**Stage 5:** full impl via Paymaster WASM contract + ADR-0032 (paymaster
design).

---

## 5. Agent JSON-RPC namespace (`agent_*`)

Stage 4 extends the JSON-RPC surface with these methods (alongside `eth_*`
and `sc_*`). Full schemas and examples are in `crates/strangecoin-api/src/
agent/` (or `src/api/agent/`) — implementation per `AI-P04`.

| Method | Stage 4 | Description |
|--------|---------|-------------|
| `agent_getSession` | full | Open a session with agent keypair auth; returns `session_id` |
| `agent_signAndSubmitBatch` | full | Submit N unsigned txs as one batch; returns `[txid; N]` |
| `agent_sponsorTx` | stub | Returns "not implemented until Stage 5" |
| `agent_estimateTaskCost` | full | Estimate gas + ACP escrow cost for a task spec |
| `agent_getActivity` | full | Query recent agent activity (transfers, batches, ACP, sponsor txs) |

### 5.1. `agent_getSession`

```json
// Request
{
  "method": "agent_getSession",
  "params": {
    "agent_pubkey": "0x...33_bytes_compressed_secp256k1...",
    "challenge_signature": "0x...64_bytes_secp256k1_over_challenge..."
  }
}

// Response
{
  "session_id": "0x...16_bytes_hex...",
  "expires_at": 1735689600,
  "scope": ["agent_signAndSubmitBatch", "agent_estimateTaskCost", "agent_getActivity"]
}
```

- `challenge` is a server-provided random 32-byte value; client signs it
  with agent keypair to prove key ownership.
- `session_id` is random u128; TTL default 1 hour (config
  `agent.session_ttl_sec`).
- Session scope defaults to all `agent_*` methods except `agent_sponsorTx`
  (stub).

### 5.2. `agent_signAndSubmitBatch`

```json
// Request
{
  "method": "agent_signAndSubmitBatch",
  "params": {
    "session_id": "0x...16_bytes_hex...",
    "txs": [
      {"type": "transfer", "to": "rsc1...", "amount": "100"},
      {"type": "call", "to": "rsc1...", "method": "doSomething", "args": [...]},
      {"type": "transfer", "to": "rsc1...", "amount": "50"}
    ]
  }
}

// Response
{
  "txids": ["0x...", "0x...", "0x..."],
  "batch_txid": "0x...",
  "status": "queued"
}
```

- Server constructs `BatchExecutor.execute(calldata_batch, signature)` tx,
  signs with session-scoped subkey (derived from agent master key via HD
  path `m/44'/1234'/{agent_id}'/1/{session_id}`).
- One on-chain tx (`batch_txid`), N internal calls (`txids` derived via
  `txid = blake3(batch_tx || index)`).
- Returns immediately with `status: "queued"`; client polls `eth_getTransactionByHash` for confirmation.

### 5.3. `agent_sponsorTx` (Stage 4 stub)

```json
// Request
{
  "method": "agent_sponsorTx",
  "params": {
    "session_id": "0x...",
    "unsigned_tx": {...},
    "paymaster_address": "rsc1..."
  }
}

// Response (Stage 4)
{
  "code": -32601,
  "message": "agent_sponsorTx not implemented until Stage 5 paymaster"
}
```

### 5.4. `agent_estimateTaskCost`

```json
// Request
{
  "method": "agent_estimateTaskCost",
  "params": {
    "session_id": "0x...",
    "task_spec": {
      "type": "acp_task",
      "agent_b": "rsc1...",
      "amount": "50",
      "deadline_blocks": 1000
    }
  }
}

// Response
{
  "gas_units": 125000,
  "fee_sc": "0.00125",
  "acp_escrow_cost_sc": "0.00075",
  "total_sc": "0.052"
}
```

- `gas_units` — estimated gas for the task's on-chain component (transfer
  or batched call + EscrowVault deposit).
- `fee_sc` — gas × fixed gas price (Stage 4; Stage 5 EIP-1559).
- `acp_escrow_cost_sc` — gas for EscrowVault deposit + settle (2 txs).
- `total_sc` — `amount` + `fee_sc` + `acp_escrow_cost_sc`.

### 5.5. `agent_getActivity`

```json
// Request
{
  "method": "agent_getActivity",
  "params": {
    "session_id": "0x...",
    "agent_address": "rsc1...",
    "filter": {
      "event_types": ["transfer", "batched_tx", "acp_negotiate", "acp_task", "acp_settle", "acp_revoke"],
      "time_range": {"from": 1735689600, "to": 1735776000},
      "limit": 50
    }
  }
}

// Response
{
  "activity": [
    {"timestamp": 1735689700, "type": "acp_settle", "agent_other": "rsc1...", "amount": "50", "txid": "0x..."},
    {"timestamp": 1735689650, "type": "acp_task", "agent_other": "rsc1...", "amount": "50", "escrow_txid": "0x..."},
    {"timestamp": 1735689600, "type": "transfer", "agent_other": "rsc1...", "amount": "100", "txid": "0x..."}
  ],
  "has_more": false
}
```

- Delegates to Stage 4 indexer (`crates/strangecoin-indexer`).
- `event_types` — see `docs/spec/acp_messages.md` for ACP types.
- `time_range` — Unix timestamp range.
- `limit` — max items per call (default 50, max 200).

---

## 6. Off-chain agent registry

There is no on-chain agent registry. Agents that want a public directory
(e.g., for reputation, capability advertisement) use the Stage 4 indexer:

- **Indexer indexes agent activity** — transfers, batched txs, ACP messages
  (NEGOTIATE/TASK/SETTLE/REVOKE), sponsor txs. Queryable via GraphQL (Stage
  4 indexer API) or via `agent_getActivity` JSON-RPC method.
- **Reputation systems are third-party.** Anyone can run an indexer that
  scores agents (e.g., "completed 100 ACP tasks on time"). Strangecoin
  ships no canonical reputation system.
- **Agent discovery is out of scope.** Agents find each other via
  out-of-band channels (off-chain directories, marketplaces, direct
  configuration). ACP negotiates once peers are connected.

---

## 7. Agent SDK

Stage 4 ships SDKs in three languages (per `AI-P05`):

- **Rust (canonical)** — `crates/strangecoin-sdk/`. Wraps `AgentClient`,
  `AgentWallet` (HD derivation, `AgentKeystore` impl), MCP-client helpers,
  ACP-client helpers.
- **TypeScript** — generated from Rust via `ts-rs` (`#[derive(TS)]` on
  shared types). Browser and Node.js compatible.
- **Python** — generated from Rust via `pyo3` (PyO3 wrapper around
  `AgentClient`). Integrates with LangChain (as a tool), AutoGen (as an
  agent), CrewAI (as a crew member).

Cookbook examples in `examples/agent_cookbook/{rust,ts,python}/`.

---

## 8. Threat mitigations (V-43..V-48)

| Vector | Mitigation | Spec section |
|--------|------------|--------------|
| V-43 MCP tool poisoning | Output determinism (#23); responses signed by node_id Ed25519 | `mcp_server.md` § Determinism, § Response signing |
| V-44 ACP spam | Stage 2 token-bucket reuse, per-message-type sub-buckets | `acp_messages.md` § Rate limit |
| V-45 ACP escrow fraud | Escrow txid verification (#24); peer ban on violation | `acp_messages.md` § Escrow verification |
| V-46 Agent key theft via MCP stdio pipe | Stdio local-only; HTTP+SSE requires Noise-XX; key isolation (#26 — `AgentKeystore` only) | This spec §3.1; `mcp_server.md` § Auth |
| V-47 UI accessibility regression | WCAG AA CI gate (#27); axe-core audit per screen | `ui_professional.md` § Accessibility |
| V-48 Sponsor paymaster DoS | Stage 5 paymaster rate limit + per-agent quota (config) | Stage 5 ADR-0032 (deferred) |

---

## 9. Anti-goals (carry from ARCHITECT.md §19.7)

These are explicitly **not** in scope:

- On-chain agent registry (consensus-breaking; agent = any EOA).
- Custom LLM-plugin protocol per vendor (MCP is the standard).
- ACP over plain TCP (Noise is required).
- Changing EOA curve for agents (secp256k1 preserved).
- Wallet-DRM / agent identity attestation (no lock-in).

---

## 10. References

- [ADR-0028](../ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md)
- [ADR-0029](../ADR/0029-mcp-server.md) + [docs/spec/mcp_server.md](mcp_server.md)
- [ADR-0030](../ADR/0030-acp-agent-communication-protocol.md) + [docs/spec/acp_messages.md](acp_messages.md)
- [ADR-0031](../ADR/0031-professional-ui-replacement.md) + [docs/spec/ui_professional.md](ui_professional.md)
- [ARCHITECT.md §19](../../analytics/ARCHITECT.md)
- [ROADMAP.md §«Этап 4»](../../analytics/ROADMAP.md)
- [prompt-stage3.md §8..§14](../../analytics/prompt-stage3.md)
- [BACKLOG.md](../../analytics/BACKLOG.md)
- [BIP-44](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki) — HD wallets
- [SLIP-44](https://github.com/satoshilabs/slips/blob/master/slip-0044.md) — coin_type registry
- [MCP](https://modelcontextprotocol.io) — Model Context Protocol
