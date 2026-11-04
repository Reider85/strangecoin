# BACKLOG.md — Сквозной бэклог Strangecoin (AI/UI + cross-stage carry-over)

**Версия:** 1.0
**Дата создания:** 2026-11-04
**Владелец:** Strangecoin maintainer
**Источник:** `analytics/prompt-stage3.md` §8..§14 (extension prompts `AI-P01`..`AI-P05`, `UI-P01`..`UI-P05`), `analytics/ROADMAP.md` §«Этап 4» (расширенный), `analytics/ARCHITECT.md` §19 (AI Agent orientation).
**Связанные ADRs:** [ADR-0028](../docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md), [ADR-0029](../docs/ADR/0029-mcp-server.md), [ADR-0030](../docs/ADR/0030-acp-agent-communication-protocol.md), [ADR-0031](../docs/ADR/0031-professional-ui-replacement.md).
**Связанные specs:** [`docs/spec/agent_integration.md`](../docs/spec/agent_integration.md), [`docs/spec/mcp_server.md`](../docs/spec/mcp_server.md), [`docs/spec/acp_messages.md`](../docs/spec/acp_messages.md), [`docs/spec/ui_professional.md`](../docs/spec/ui_professional.md).

**Назначение:** single source of truth для AI-agent-track и professional-UI-track work items. До этого патча «бэклог Stage 3» жил в `ROADMAP3.md` (referenced в `prompt-stage3.md` §0 Замечание про BACKLOG3.md), «бэклог Stage 4» — в `ROADMAP.md` §«Этап 4» (de-facto). Теперь AI/UI work items живут здесь для traceability.

**Conventions:**
- Status: `queued` (not started) / `in-progress` (assigned, working) / `blocked` (dependency) / `done` (closed, commit linked) / `deferred` (Stage 5+ carry-over).
- Owner: GitHub handle или `unassigned`.
- ETA: YYYY-MM-DD или `--` (unknown).
- Priority: `P0` (blocker) / `P1` (high) / `P2` (medium) / `P3` (low).

---

## 1. Active work items (in-progress / queued)

| ID | Title | Status | Owner | ETA | Priority | Depends on | ADR | Spec |
|----|-------|--------|-------|-----|----------|------------|-----|------|
| AI-P01 | ADR-0028: Strangecoin as cryptocurrency for AI agents (vision + account model) | queued | unassigned | -- | P1 | (none, design-only) | 0028 | agent_integration.md |
| AI-P02 | ADR-0029 + MCP server scaffold (7 tools, 4 resources, stdio + HTTP+SSE transports) | queued | unassigned | -- | P1 | AI-P01 | 0029 | mcp_server.md |
| AI-P03 | ADR-0030 + ACP wire messages (NEGOTIATE/TASK/SETTLE/REVOKE) + EscrowVault + replay protection | queued | unassigned | -- | P1 | AI-P01 | 0030 | acp_messages.md |
| AI-P04 | Agent JSON-RPC surface (agent_getSession / signAndSubmitBatch / sponsorTx stub / estimateTaskCost / getActivity) | queued | unassigned | -- | P1 | AI-P03 (escrow txid) | (none, extends 0028) | agent_integration.md |
| AI-P05 | Agent SDK (Rust + TS + Python) + agent devnet + faucet + 3-agent e2e | queued | unassigned | -- | P1 | AI-P04 | (none, extends 0028) | (none) |
| UI-P01 | ADR-0031: Professional UI replacement (render backend + design system principles) | queued | unassigned | -- | P1 | (none, design-only) | 0031 | ui_professional.md |
| UI-P02 | Design system + 14 components + tokens + themes + i18n + a11y audit (`crates/strangecoin-ui/`) | queued | unassigned | -- | P1 | UI-P01 | (none, extends 0031) | ui_professional.md |
| UI-P03 | Wallet screen migration (send / history / contracts) + strangler launcher | queued | unassigned | -- | P1 | UI-P02 | (none) | ui_professional.md |
| UI-P04 | Node operator dashboard + transaction detail (receipts / witness proofs) | queued | unassigned | -- | P1 | UI-P02 | (none) | ui_professional.md |
| UI-P05 | Agent activity feed + settings + a11y audit + strangler closeout + Changelog 4.0.0 | queued | unassigned | -- | P1 | UI-P03, UI-P04, AI-P04 (agent_getActivity) | (none) | ui_professional.md |

**Зависимости (карта):** см. `prompt-stage3.md` §8.2.

---

## 2. Next up (after current 10 prompts)

| ID | Title | Stage | Notes |
|----|-------|-------|-------|
| (Stage 4 core) | JSON-RPC eth_* + HD wallet + SDK core + devnet + explorer + faucet + indexer + metrics | 4 | Из оригинального ROADMAP.md «Этап 4» (не extension); parallel track к AI/UI |
| S4-core-P01 | `crates/strangecoin-api/` (JSON-RPC server) | 4 | Dep: Stage 3 closed |
| S4-core-P02 | HD Wallet (BIP-39/44) | 4 | Dep: none |
| S4-core-P03 | `cargo-strangecoin` SDK (foundry-аналог) | 4 | Dep: S4-core-P01 |
| S4-core-P04 | Local devnet (anvil-аналог) | 4 | Dep: S4-core-P02 |
| S4-core-P05 | Block explorer (web-based) | 4 | Dep: S4-core-P01 |
| S4-core-P06 | Testnet faucet | 4 | Dep: S4-core-P04 |
| S4-core-P07 | Indexer (Subgraph/sqd-аналог) | 4 | Dep: Stage 3 closed; extension: indexes agent activity for AI-P04 `agent_getActivity` |
| S4-core-P08 | Metrics (Prometheus) | 4 | Dep: none |

---

## 3. Blocked

| ID | Title | Blocked on | Notes |
|----|-------|------------|-------|
| AI-P02 | MCP server scaffold | Stage 3 tag `v3.0.0-stage3` (hard gate §8.3 #1) + AI-P01 ADR-0028 (hard gate §8.3 #2) | ADR + spec can be written during Stage 3 (design-only artefacts); code after tag |
| AI-P03 | ACP transport | Stage 3 tag + AI-P01 ADR-0028 | Same as above |
| AI-P04 | Agent JSON-RPC | AI-P03 (needs ACP escrow txid) | |
| AI-P05 | Agent SDK + devnet | AI-P04 | |
| UI-P02 | Design system crate | UI-P01 ADR-0031 | |
| UI-P03 | Wallet screen | UI-P02 | |
| UI-P04 | Dashboard + Tx Detail | UI-P02 | |
| UI-P05 | Agent Activity + Settings + closeout | UI-P03, UI-P04, AI-P04 | |

---

## 4. Done (closed prompts)

| ID | Title | Commit | Close date | Notes |
|----|-------|--------|------------|-------|
| (none yet) | | | | Stage 4 not started; v2.0.0-stage2 is the latest tag |

---

## 5. Stage 4 residual obligations (carry-over to Stage 5+)

| Item | Stage | Reason | ADR/SCIP needed |
|------|-------|--------|-----------------|
| Paymaster full impl (`agent_sponsorTx` real) | 5 | Stage 4 ships stub returning "not implemented" | ADR-0032 (Stage 5 paymaster design) |
| Account Abstraction protocol-level | 5 | Stage 4 ships batched tx via WASM contract only (no AA protocol) | SCIP-0002 (AA activation height) |
| EIP-1559 fee market | 5 | Stage 4 uses fixed gas price; `agent_estimateTaskCost` based on fixed | SCIP-0003 (EIP-1559 activation) |
| Mobile UI (iOS/Android) | 5+ | Stage 4 desktop only (retain eframe/egui per ADR-0031) | ADR-0033 (mobile UI strategy — Tauri Mobile vs React Native vs native) |
| Multi-language SDK (Go, Java, C++) | 5+ | Stage 4 — Rust + TS + Python only | (no ADR yet; deferred) |
| Wire ACP-over-gossip (relay через third peer) | 5+ research | Stage 4 — direct peer-to-peer only | ADR-0034 (relayed ACP design) |
| ZK-proofs for ACP escrow | 6+ research | Stage 4 — on-chain txid verification only | ADR-0035 (ZK-ACP) |
| ACP formal spec → TLA+ model | 6+ | Stage 4 — informal spec (`docs/spec/acp_messages.md`) | (TLA+ network spec is Stage 6 ROADMAP item) |
| MCP spec version upgrade (post-2024-11-05) | 5+ | MCP spec churn risk; pin to 2024-11-05 in ADR-0029 | ADR-0029 amendment when MCP spec stabilizes |
| egui accessibility maturity migration | 5+ | If egui adds native screen reader hooks → adopt; else Stage 5+ consider Tauri+Web migration | ADR-0031 amendment or new ADR-0036 |
| Threshold encryption mempool (MEV mitigation) | 5 | ROADMAP Этап 5 | ADR (Stage 5 track) |
| State rent (optional) | 3+/SCIP-gated | ARCHITECT.md §16 open question #5 | SCIP (consensus-changing) |

---

## 6. Cross-stage carry-over (not Stage 4 — listed for visibility)

| Item | Stage | Status | Notes |
|------|-------|--------|-------|
| **SCIP-0001** — replace burned genesis key | 0/mainnet freeze | BLOCKING mainnet | `docs/SCIP/scip-0001-genesis-key-replacement.md`; air-gapped tooling in `examples/`; awaits human ceremony |
| **BUG-S1-013/015** — ops-gates | 0/mainnet freeze | Carry-over | Outside Stage 3/4 scope; tracked in `docs/stage0/CRITICAL_ISSUES_CLOSED.md` |
| Stage 3 residuals: Verkle trie замена SMT | SCIP-gated | Deferred | ADR-0006 amendment; consensus-breaking |
| Stage 3 residuals: State expiry (dormant account removal) | SCIP-gated | Deferred | ADR-0025 marker-only Stage 3; full impl post-Stage 3 via SCIP |
| Stage 3 residuals: Receipts prune / contract storage prune | SCIP-gated | Deferred | Residual Stage 1.5/2 |
| Stage 3 residuals: Block-STM (parallel execution) | 3+/Stage 6 | Deferred | Differential serial-vs-parallel separate track |
| Stage 3 residuals: Wire snapshot sync (GET_SNAPSHOT P2P) | 4+ | Deferred | S3-P13 file-based only Stage 3 |
| Stage 3 residuals: Wire witness integration в gossip/sync | 4+ | Deferred | S3-P11 node API only Stage 3 |
| Stage 3 residuals: Персистентный SMT (trie-узлы в DB) | Residual ADR-0024 | Deferred | invariant #1 rebuild preserved Stage 3 |
| Stage 3 residuals: Erlay full minisketch impl (impl_id=1) | SCIP-gated | Deferred | Carry-over Stage 2 |
| Wallet перенос в отдельный крейт | 4+ | Stage 4 (UI-P02..UI-P05 partial via `crates/strangecoin-ui/`) | ARCHITECT.md §10.5 mentions; full wallet crate Stage 5+ |
| PoS migration (BLS12-381, slashing, validator set) | 7 | PLANNED | ROADMAP Этап 7; ADR-0003 baseline |
| External security audit | 6 | PLANNED | ROADMAP Этап 6 |
| Formal verification of consensus rules | 6 | PLANNED | ROADMAP Этап 6 |

---

## 7. Process

1. **Update cadence:** BACKLOG.md обновляется при каждом коммите `AI-P##` / `UI-P##` (move from §1 to §4 with commit hash).
2. **Review cadence:** weekly review of §3 (Blocked) — unblock what's ready; §5 (Residual) — promote to §1 when target stage opens.
3. **Cross-link:** every PR closing an `AI-P##` / `UI-P##` references this file in commit message (`Updates analytics/BACKLOG.md §1: AI-P02 done → §4`).
4. **No orphan items:** every item must have an ADR (if architectural) or spec (if protocol) or both — never just a line in BACKLOG.
5. **No silent deferral:** if an item moves from §1 to §5 (Residual), commit message must explain why (e.g., "Stage 4 scope too large; deferred to Stage 5 per ADR-0032 paymaster design").

---

*Файл создан 2026-11-04 вместе с расширением `prompt-stage3.md` §8..§14. Поддерживается в актуальном состоянии по мере закрытия промптов.*
