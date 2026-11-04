# Strangecoin AI-Agent Patch — Summary of Changes

**Date:** 2026-11-04
**Repo:** https://github.com/Reider85/strangecoin
**Patch file:** `strangecoin-ai-agent-patch.patch` (apply with `git apply`)

## Goal

Position Strangecoin as a **cryptocurrency for AI agents** by adding:
- AI-agent interaction prompts (MCP, ACP) to `prompt-stage3.md`
- Prompts to replace the current `eframe`/`egui` demo UI with a professional UI
- New ADRs (0028..0031) and spec files for AI agent integration, MCP server, ACP messages, and professional UI
- Cross-stage `BACKLOG.md` for traceability
- Updates to `README.md`, `analytics/ARCHITECT.md`, `analytics/ROADMAP.md` declaring the AI-agent orientation

## Files modified (5)

| File | Was → Now | Change |
|------|-----------|--------|
| `README.md` | 566 → 584 lines | Added "cryptocurrency for AI agents" tagline, AI-agent orientation callout, "AI-Agent Ready" badge, "AI agents" feature subsection (Stage 3+ / Stage 4 target), updated roadmap row Stage 4 "+ AI agents", project structure references to new ADRs/specs/BACKLOG.md, acknowledgements footer mentions MCP/ACP |
| `analytics/ARCHITECT.md` | 995 → 1171 lines | Added AI-agent orientation to intro strategic decisions; added §19 "AI Agent orientation" with: principles (§19.1), context diagram (§19.2), functional subsystems (§19.3 — MCP server, ACP transport, Agent JSON-RPC, Agent SDK, Professional UI, Agent devnet), workspace map (§19.4), invariants #23..#27 (§19.5), threat vectors V-43..V-48 (§19.6), anti-goals (§19.7), open questions (§19.8) |
| `analytics/ROADMAP.md` | 1262 → 1371 lines | Added AI-agent orientation to intro; renamed Stage 4 to "Dev-experience + AI agents"; added task blocks for MCP server (ADR-0029), ACP transport (ADR-0030), Agent JSON-RPC, Agent SDK, Agent devnet/faucet, Professional UI (ADR-0031); extended DoD with 7 AI/UI items; extended Risks with 5 AI/UI risks; extended ADRs required with 0028..0031 |
| `analytics/prompt-stage3.md` | 839 → 1569 lines | **Preserved all original S3-P01..S3-P15 prompts (lines 1-838 unchanged)**; appended §8 "Extension prompts" (overview, shared context, dependency map, hard gates, anti-goals); §9 AI-P01..AI-P05 (agent integration, MCP, ACP, agent JSON-RPC, agent SDK); §10 UI-P01..UI-P05 (professional UI design system, wallet screen, dashboard + tx detail, agent activity + settings + closeout); §11 DoD coverage for extension; §12 BACKLOG.md reference; §13 cross-references; §14 Stage 4 gate closure |
| `docs/spec/README.md` | 215 → 276 lines | Added "AI Agent integration specs" section referencing new spec files and mapping to ARCHITECT.md §19 invariants/threats |

## Files created (9)

### Backlog
| File | Lines | Purpose |
|------|-------|---------|
| `analytics/BACKLOG.md` | 128 | Cross-stage backlog: active work items (AI-P01..AI-P05, UI-P01..UI-P05), next-up Stage 4 core, blocked items, done items, Stage 4 residual obligations, cross-stage carry-over (SCIP-0001, BUG-S1-013/015, Stage 3 residuals), process rules |

### ADRs (4 new — Proposed status)
| ADR | File | Lines | Decision |
|-----|------|-------|----------|
| 0028 | `docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md` | 253 | Agent identity = wallet identity (secp256k1 EOA, no on-chain registry); HD derivation `m/44'/1234'/agent_id'`; key isolation (`OperatorKeystore` vs `AgentKeystore`); session keys; batched tx via WASM contract; sponsored tx (Stage 5); off-chain registry; agent_* JSON-RPC namespace |
| 0029 | `docs/ADR/0029-mcp-server.md` | 250 | MCP server in `crates/strangecoin-mcp/` — 7 tools, 4 resources, stdio + HTTP+SSE transports; output determinism (#23); Ed25519 response signing (V-43 mitigation); Noise-XX auth reuse for HTTP+SSE |
| 0030 | `docs/ADR/0030-acp-agent-communication-protocol.md` | 302 | ACP wire messages (`ACP_NEGOTIATE`/`ACP_TASK`/`ACP_SETTLE`/`ACP_REVOKE`) over Noise P2P; on-chain `EscrowVault` WASM contract for escrow; replay protection monotonic nonces (#25); rate limit reuse Stage 2; peer scoring extensions |
| 0031 | `docs/ADR/0031-professional-ui-replacement.md` | 330 | Retain eframe/egui 0.22+ as render backend; new `crates/strangecoin-ui/` (design system + 14 components + 5 screens); strangler migration `src/gui/` → launcher; WCAG AA CI gate (#27); dark/light themes; i18n (en/ru/zh) via `fluent` |

### Specs (4 new)
| Spec | File | Lines | Content |
|------|------|-------|---------|
| `agent_integration.md` | 440 | High-level agent integration: identity, HD path, key isolation, session keys, batched/sponsored tx patterns, agent_* JSON-RPC method schemas, off-chain registry, threat mitigations |
| `mcp_server.md` | 509 | MCP server spec: 7 tool schemas (get_block, get_balance, get_nonce, submit_tx, call_contract, estimate_fee, subscribe_events), 4 resource URIs, transports (stdio + HTTP+SSE), determinism, response signing, lifecycle, crate structure, test plan |
| `acp_messages.md` | 483 | ACP spec: 4 message schemas, wire format, escrow flow diagrams (happy path, timeout, mutual cancel), escrow verification (#24), replay protection (#25), rate limit (V-44), `EscrowVault` WASM contract ABI, peer scoring extensions, test plan |
| `ui_professional.md` | 593 | UI spec: design tokens (color palette 7 hues × 10 shades, spacing scale, typography scale, radius, shadow), themes, i18n, 14 components, 5 screens (Wallet/Dashboard/Tx Detail/Agent Activity/Settings) with wireframes, WCAG AA checklist, strangler migration plan, crate structure |

## New prompts added to `prompt-stage3.md`

All new prompts are in §8..§14, **preserving the original 838 lines (S3-P01..S3-P15) untouched**.

| ID | Title | Stage |
|----|-------|-------|
| AI-P01 | ADR-0028 + agent account model | 4 (design during Stage 3) |
| AI-P02 | ADR-0029 + MCP server scaffold (7 tools, 4 resources, stdio + HTTP+SSE) | 4 |
| AI-P03 | ADR-0030 + ACP wire messages + EscrowVault + replay protection | 4 |
| AI-P04 | Agent JSON-RPC (agent_getSession, signAndSubmitBatch, sponsorTx stub, estimateTaskCost, getActivity) | 4 |
| AI-P05 | Agent SDK (Rust + TS + Python) + agent devnet + faucet + 3-agent e2e | 4 |
| UI-P01 | ADR-0031 (render backend + design system principles) | 4 (design during Stage 3) |
| UI-P02 | Design system + 14 components + tokens + themes + i18n + a11y audit | 4 |
| UI-P03 | Wallet screen migration (Send/History/Contracts) + strangler launcher | 4 |
| UI-P04 | Node dashboard + tx detail (receipts, witness proofs) | 4 |
| UI-P05 | Agent activity + settings + a11y audit + strangler closeout + Changelog 4.0.0 | 4 |

## How to apply

### Option A: Apply patch from repo root

```bash
cd strangecoin
git apply strangecoin-ai-agent-patch.patch
```

### Option B: Copy individual files

Copy files from `strangecoin-ai-agent-patch/` to the corresponding paths in your local clone of `https://github.com/Reider85/strangecoin`.

## Verification checklist

- [x] Original `prompt-stage3.md` lines 1-838 (S3-P01..S3-P15) preserved unchanged
- [x] New §8..§14 appended with 10 new prompts (AI-P01..AI-P05, UI-P01..UI-P05)
- [x] `README.md` updated with AI-agent positioning + badge + feature subsection
- [x] `analytics/ARCHITECT.md` extended with §19 AI Agent orientation (invariants #23..#27, threats V-43..V-48)
- [x] `analytics/ROADMAP.md` Stage 4 extended with AI/MCP/ACP/UI tasks, DoD, risks, ADRs required
- [x] `analytics/BACKLOG.md` created with cross-stage work item tracking
- [x] ADRs 0028..0031 created following ADR-0001 template (Status/Context/Decision/Consequences/Alternatives/Related)
- [x] Spec files created for agent_integration / mcp_server / acp_messages / ui_professional
- [x] `docs/spec/README.md` extended with AI Agent integration specs section
- [x] All cross-references between ADRs / specs / ARCHITECT / ROADMAP / prompts / BACKLOG consistent

## What was NOT changed (out of scope)

- Source code (`src/`, `crates/`) — Stage 4 implementation requires the `v3.0.0-stage3` tag first (hard gate §8.3 #1 in `prompt-stage3.md`)
- ADRs 0001..0027 — unchanged
- Original Stage 3 prompts S3-P01..S3-P15 — preserved verbatim
- `Changelog.md` — Stage 4 release notes will be added by UI-P05 closeout
- `AGENTS.md` — Stage 4 sections will be added by UI-P05 closeout
- `docs/security/THREAT_MODEL.md` — STRIDE v3.3 → v3.4 update deferred to UI-P05 closeout or separate security-patch
