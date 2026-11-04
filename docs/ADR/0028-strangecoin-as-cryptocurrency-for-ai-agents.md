# ADR-0028: Strangecoin as Cryptocurrency for AI Agents

## Status

Proposed (2026-11-04) — `AI-P01` (extension prompt in `analytics/prompt-stage3.md` §9). 
Acceptance gate: land of `AI-P02` (MCP server scaffold) confirms the account-model decisions are workable.

## Context

Strangecoin today (verified at HEAD post-Stage 2, tag `v2.0.0-stage2`) is a generic
Layer-1 cryptocurrency: PoW + secp256k1 EOA + bech32 + Sparse Merkle state root +
WASM VM (Stage 1.5) + Noise-authenticated P2P (Stage 2). The base layer treats
every account identically — there is no concept of "agent" versus "human" at the
consensus level.

Several forces are pulling Strangecoin toward an explicit AI-agent orientation:

1. **Market pull.** Model Context Protocol (MCP) and Agent Communication Protocol
   (ACP) frameworks have matured (2024-2025). LLM vendors (Anthropic, OpenAI,
   Google) and open-weight agent runtimes (LangChain, AutoGen, CrewAI) now expect
   a stable tool-calling surface and peer-to-peer agent messaging. Cryptocurrencies
   that ship native MCP/ACP support have a structural advantage for agent
   onboarding over those that require ad-hoc JSON-RPC wrapping per consumer.
2. **Stage 1.5 already delivers the prerequisites.** `wasmi` VM + EOAs + contract
   accounts + 13 host functions + JSON ABI means Strangecoin can express
   batched transactions, programmable policy wallets (multisig / session-key
   validators), and sponsored transactions (paymaster pattern) **as WASM
   contracts — without any consensus change**. The agent layer can be built
   entirely off the existing base layer.
3. **Stage 2 already delivers the transport.** Noise XX + ChaCha20-Poly1305
   peer-to-peer with peer scoring and rate limiting means agent-to-agent
   messaging (ACP) can ride the existing transport — no new crypto, no new
   network stack.
4. **Documentation gap.** The current `README.md` and `analytics/ARCHITECT.md`
   describe Strangecoin as a "production-grade Layer-1" with no first-class
   agent story. This makes agent-runtime consumers (LangChain, etc.) see
   Strangecoin as just another EVM-like chain — missing the structural
   differentiator.

The competing forces in tension:

- **Differentiation vs. compatibility.** Agent-native features (MCP, ACP) should
  differentiate Strangecoin, but the secp256k1 EOA + bech32 + JSON ABI surface
  must stay MetaMask/Ledger-compatible (per ADR-0001).
- **On-chain vs. off-chain.** Agent identity, reputation, and ACP message
  routing can be on-chain (consensus-breaking, gas-expensive) or off-chain
  (indexer-trusted, micropayment-friendly). The right boundary is non-obvious.
- **Stage 4 scope vs. Stage 5+.** Some agent features (full paymaster, full
  account abstraction, EIP-1559) belong in Stage 5; Stage 4 must ship a
  coherent subset that is useful on its own.

## Decision

**We will position Strangecoin as a cryptocurrency for AI agents, with the
following architectural commitments:**

1. **Agent identity = wallet identity.** An agent is a `secp256k1` EOA owned by
   an agent runtime (not by a human operator). No new account type, no on-chain
   agent registry. The consensus layer treats agents identically to human
   wallets — agent-ness is a property of how the key is used, not what the chain
   records. This preserves MetaMask/Ledger compatibility (ADR-0001) and avoids
   consensus changes.

2. **HD derivation for sub-agents and session keys.** Agents derive sub-keys
   via BIP-44 path `m/44'/1234'/<agent_id>'/0/<sub_id>` (sub-agents) and
   `m/44'/1234'/<agent_id>'/1/<session_id>` (rotating session keys with short
   TTL). `coin_type = 1234` is a placeholder; SLIP-44 registration is a Stage 4
   deliverable (`AI-P05` opens the PR; if rejected, placeholder is documented).

3. **Key isolation: `OperatorKeystore` vs `AgentKeystore`.** The wallet trait is
   split into two non-overlapping types: `OperatorKeystore` (access to the
   node-operator's wallet, used for block production / fee collection / signing
   consensus messages) and `AgentKeystore` (access to agent keys, sandboxed).
   MCP handlers (`AI-P02`) and ACP handlers (`AI-P03`) have compile-time access
   only to `AgentKeystore` — never to `OperatorKeystore`. This is enforced via
   the Rust type system (separate types, no shared trait object that exposes
   both). Invariant #26 (ARCHITECT.md §19.5).

4. **Session-key pattern (optional).** An agent may derive a short-TTL session
   keypair, scoped to specific contracts / methods / max-value, signed by the
   master agent key. The session key is validated by an on-chain WASM policy
   wallet (Stage 1.5 contract). This pattern lets an agent delegate limited
   authority to a sub-process (e.g., a sandboxed tool runner) without exposing
   the master key.

5. **Batched transactions via WASM contract.** One signature, N contract calls
   — implemented as a WASM contract `BatchExecutor` that takes signed calldata
   batch and executes N internal `call`s. No consensus change. Surfaced in
   JSON-RPC as `agent_signAndSubmitBatch` (`AI-P04`) and in MCP as a helper
   that wraps `submit_tx` for the batch contract.

6. **Sponsored transactions (paymaster).** Stage 5 full impl; Stage 4 ships
   `agent_sponsorTx` as a stub returning `{"code": -32601, "message": "not
   implemented until Stage 5 paymaster"}`. Paymaster contract pattern (WASM
   contract pays gas for agent tx; agent signs intended action; paymaster
   signs sponsorship) is the design direction — deferred to Stage 5 + ADR-0032.

7. **Off-chain agent registry.** The Stage 4 indexer (`crates/strangecoin-indexer`)
   indexes agent activity (transfers, batched txs, ACP messages, sponsor txs)
   off-chain. The `agent_getActivity` JSON-RPC method (`AI-P04`) and the
   Agent Activity UI screen (`UI-P05`) read from the indexer. No on-chain
   registry — agents are anonymous EOAs, like any other; reputation is an
   off-chain concern (indexer, optional).

8. **Agent-native JSON-RPC namespace.** Stage 4 JSON-RPC surface extends with
   `agent_*` methods (alongside `eth_*` for Ethereum tooling interop and
   `sc_*` for Strangecoin-specific): `agent_getSession`,
   `agent_signAndSubmitBatch`, `agent_sponsorTx` (stub Stage 4),
   `agent_estimateTaskCost`, `agent_getActivity`. See `AI-P04` for details.

9. **MCP server as first-class LLM facade.** Stage 4 ships a Model Context
   Protocol server (`crates/strangecoin-mcp/`, ADR-0029, `AI-P02`) exposing
   chain state as MCP tools and resources. Any MCP-capable LLM (Claude, GPT,
   Gemini, open-weight via `mcp-cli`) reads blocks, queries balances, submits
   transactions, and subscribes to events without a custom plugin. MCP is a
   **parallel facade** to JSON-RPC — not a replacement.

10. **ACP (Agent Communication Protocol) over Noise.** Stage 4 ships a wire
    message family `ACP_NEGOTIATE` / `ACP_TASK` / `ACP_SETTLE` / `ACP_REVOKE`
    riding on the existing Noise-authenticated P2P transport (ADR-0020). Agents
    negotiate tasks, quote and pay for services, and settle micropayments
    peer-to-peer — without a centralized marketplace. Escrow references on-chain
    txid (verified via `StorageEngine::get_tx(txid)`, invariant #24). See
    ADR-0030, `AI-P03`.

11. **Professional UI replaces demo egui wallet.** Stage 4 ships a new crate
    `crates/strangecoin-ui/` (design system + component library + 5 screens)
    replacing the current `eframe`/`egui` demo wallet. The existing `src/gui/`
    becomes a thin launcher (strangler pattern, per ARCHITECT.md §10.5).
    Migration governed by ADR-0031. See `UI-P01`..`UI-P05`.

### Stage 4 scope boundary

**In Stage 4 (this ADR's actionable scope):** ADRs 0028..0031, MCP server
(AI-P02), ACP transport (AI-P03), `agent_*` JSON-RPC (AI-P04), agent SDK
Rust+TS+Python + devnet + faucet + multi-agent e2e (AI-P05), professional UI
5 screens (UI-P01..UI-P05).

**Deferred to Stage 5:** paymaster full impl, account abstraction protocol-level,
EIP-1559 fee market (Stage 4 `agent_estimateTaskCost` uses fixed gas price).

**Deferred to Stage 5+ research:** mobile UI (iOS/Android), multi-language SDK
(Go/Java/C++), wire ACP-over-gossip (relay via third peer), ZK-proofs for ACP
escrow, threshold encryption mempool.

**Never (anti-goals, ARCHITECT.md §19.7):** on-chain agent registry, custom
LLM-plugin protocol per vendor, ACP over plain TCP (without Noise), changing
EOA curve for agents, wallet-DRM / agent identity attestation.

## Consequences

### Positive

- **No consensus change.** All agent features are expressible on the existing
  Stage 0..3 base layer (WASM contracts + Noise transport + JSON-RPC).
- **MetaMask/Ledger compatibility preserved.** Agents use the same secp256k1
  curve and bech32 address format as human wallets.
- **MCP/ACP ride existing transport.** No new crypto, no new network stack —
  the Noise-XX handshake (ADR-0020) and ChaCha20-Poly1305 transport are reused.
- **SDK reuses existing wallet primitives.** The Stage 4 SDK (AI-P05) wraps the
  existing wallet module rather than reimplementing key management.
- **Differentiation.** Strangecoin becomes a credible "agent-native L1" without
  abandoning the EVM-ish tooling ecosystem.
- **Audit-ready.** All agent code lives in new crates (`strangecoin-mcp`,
  `strangecoin-sdk`, `strangecoin-ui`) with clear boundaries — easier to audit
  than bolted-on features inside existing crates.

### Negative

- **HD-derivation adds key-management complexity for agent runtimes.** Agents
  must correctly derive and rotate session keys; bugs here are security
  incidents. Mitigation: SDK (AI-P05) ships well-tested helpers; cookbook
  examples show canonical patterns.
- **Session-key WASM contract needs audit.** The `BatchExecutor` and
  session-key validator contracts are Stage 1.5 WASM code — they must be
  audited (Stage 6 external audit covers this).
- **Off-chain indexer becomes trust-source for agent reputation.** Stage 4
  indexer is run by node operators; reputation is their call. Mitigation:
  multiple indexers can be run; agents can query several; on-chain txid is
  always the ground truth (ACP escrow verification #24).
- **Stage 4 scope grows.** Adding MCP + ACP + agent SDK + professional UI to
  Stage 4 (alongside the original SDK/devnet/indexer/explorer/faucet/JSON-RPC
  eth_*) is a large body of work. Mitigation: extension prompts AI-P## / UI-P##
  are gated (§8.3) and can slip to Stage 4.x if needed without blocking
  Stage 4 core.
- **More attack surface.** MCP server (V-43), ACP transport (V-44, V-45), agent
  key handling (V-46), UI accessibility (V-47), sponsor paymaster (V-48) are
  new threat vectors (ARCHITECT.md §19.6). Mitigation: each has documented
  mitigations and is regression-tested.

### Neutral

- **Agent reputation is out of scope.** Strangecoin ships the primitives
  (on-chain tx history, ACP message log via indexer); reputation systems
  (off-chain scoring, attestation services) are third-party concerns.
- **Indexer can be swapped.** The `agent_getActivity` JSON-RPC method delegates
  to whatever indexer the node operator runs; the spec does not mandate a
  specific indexer implementation.

## Alternatives

### Alternative 1: On-chain agent registry

- **Pros:** Public, verifiable agent directory; reputation can be on-chain.
- **Cons:** Consensus-breaking (new account type); gas cost per registry
  operation; privacy loss (agents cannot be anonymous EOAs); significant scope
  creep.
- **Why not chosen:** Anti-goal (ARCHITECT.md §19.7). Agent = any EOA is a
  stronger primitive — agents that want a public directory can build one
  off-chain (indexer).

### Alternative 2: Custom LLM-plugin protocol per vendor

- **Pros:** Tight integration with each LLM vendor's idioms.
- **Cons:** Lock-in; N protocols to maintain; loses ecosystem interop.
- **Why not chosen:** Anti-goal (§19.7). MCP is the emerging open standard;
  one implementation serves all MCP-capable LLMs.

### Alternative 3: Separate agent L2 / sidechain

- **Pros:** Agent features can be consensus-breaking on the L2 without
  affecting L1.
- **Cons:** Bridge risk (L1↔L2 asset transfer); fragmentation of liquidity;
  additional protocol complexity; Stage 6+ research scope.
- **Why not chosen:** Stage 4 should ship on L1 to maximize adoption. L2 is
  a Stage 6+ research item, not a Stage 4 deliverable.

### Alternative 4: Account Abstraction at protocol level (Stage 5)

- **Pros:** Cleanest paymaster / session-key / policy-wallet story; no need for
  WASM contract wrappers.
- **Cons:** Consensus-breaking; significant protocol complexity; not Stage 4
  scope.
- **Why not chosen:** Stage 4 ships batched tx via WASM contract (sufficient
  for most agent use cases). AA protocol-level is Stage 5 (`SCIP-0002`).

## Related

- [ADR-0001](0001-secp256k1-vs-ed25519.md) — secp256k1 for EOA (preserved; agents use same curve)
- [ADR-0008](0008-rocksdb-plan.md) — storage migration (Stage 3 prerequisite for indexer performance)
- [ADR-0020](0020-noise-protocol-framework.md) — Noise transport (reused for ACP)
- [ADR-0022](0022-peer-scoring-algorithm.md) — peer scoring (extended for ACP violations)
- [ADR-0027](0027-license-change-to-apache-2.0.md) — Apache-2.0 license
- [ADR-0029](0029-mcp-server.md) — MCP server design (AI-P02)
- [ADR-0030](0030-acp-agent-communication-protocol.md) — ACP design (AI-P03)
- [ADR-0031](0031-professional-ui-replacement.md) — Professional UI (UI-P01)
- [ARCHITECT.md §19](../../analytics/ARCHITECT.md) — AI Agent orientation (principles, context diagram, invariants #23..#27, threat vectors V-43..V-48, anti-goals, open questions)
- [ROADMAP.md §«Этап 4»](../../analytics/ROADMAP.md) — Stage 4 (extended with AI/MCP/ACP/UI tasks)
- [prompt-stage3.md §8..§14](../../analytics/prompt-stage3.md) — Extension prompts AI-P01..AI-P05, UI-P01..UI-P05
- [BACKLOG.md](../../analytics/BACKLOG.md) — Cross-stage backlog (AI/UI work items)
- [docs/spec/agent_integration.md](../spec/agent_integration.md) — Agent integration spec
- [Model Context Protocol](https://modelcontextprotocol.io) — MCP standard (spec version 2024-11-05)
- [SLIP-44](https://github.com/satoshilabs/slips/blob/master/slip-0044.md) — BIP-44 coin_type registry (placeholder 1234, registration pending AI-P05)
