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
| AI-P04 | Agent JSON-RPC surface (agent_getSession / signAndSubmitBatch / sponsorTx stub / estimateTaskCost / getActivity) | queued | unassigned | -- | P1 | AI-P03 (escrow txid) | (none, extends 0028) | agent_integration.md |
| AI-P05 | Agent SDK (Rust + TS + Python) + agent devnet + faucet + 3-agent e2e | queued | unassigned | -- | P1 | AI-P04 | (none, extends 0028) | (none) |
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
| AI-P05 | Agent SDK + devnet | AI-P04 | |
| UI-P04 | Dashboard + Tx Detail | UI-P02 | |
| UI-P05 | Agent Activity + Settings + closeout | UI-P03, UI-P04, AI-P04 | |

---

## 4. Done (closed prompts)

| ID | Title | Commit | Close date | Notes |
|----|-------|--------|------------|-------|
| GITHUB-P35 | Nightly audit unblocked (run #20) + bounded miri/mutation/fuzz signals | `[GITHUB-P35] (this commit)` | 2026-11-27 | Root cause (job `Audit`, exit 5/1): `RUSTSEC-2026-0592` (`url 2.5.8` stack overflow in `Url::origin()`, patched in 2.5.9) + `error[unlicensed]: strangecoin-ui = 0.1.0 is unlicensed`. Fix: `cargo update -p url --precise 2.5.9` in root AND fuzz lockfiles; `license = "Apache-2.0"` added to `crates/strangecoin-ui/Cargo.toml` — **no new baseline ignores**, no lint disabled. The prompt's quick-xml hypothesis was wrong (0194/0195 already baselined in [UI-P02-B]). Miri split (vm-wasm first, storage `--lib`) so vm-wasm actually runs (issue #14); mutation bounded to the 15-mutant `chain_selector.rs` file after both `--workspace` (6387 mutants) and `-p strangecoin-core` (1175 mutants) exceeded the 50-min cap; two fuzz targets fixed at compile level (`#![no_main]`, `libfuzzer_sys::fuzz_target` import, `serde_json` dep, `secp256k1` moved out of the `[dependencies.strangecoin-net]` table). Deviations 11–14 in the `nightly.yml` header |
| UI-P03 | Wallet screen migration (send / history / contracts) + strangler launcher | `[S4-UI-P03]` (this commit) | 2026-11-26 | `crates/strangecoin-ui/src/backend.rs` (`WalletBackend` seam + `MockWalletBackend`) + `src/screens/wallet.rs` (3 tabs on real data, typed `Table` cells, filter/pagination, ABI viewer, honest batch refusal) + `src/api/mod.rs` (in-process `InProcessWalletBackend` over `BlockchainFacade`; Stage-0 placeholder filled) + shared read-only VM path extracted into `src/blockchain/read_only.rs` (MCP + wallet) + `Typed cell_renderer` on `Table<T>` + 18 new i18n keys × en/ru/zh (152 total) + `tests/screens/wallet_a11y.rs` + `tests/wallet_backend.rs`; launcher active by default (Phase3) with real mining sender. Deviations in ADR-0031 Amendment C: no `eth_*` HTTP server exists (AI-P04 queued), so JSON-RPC names stay as a trait seam; fee=0 until Stage 5; batch = `NotImplemented`; ABI empty until indexer S4-core-P07; legacy `WalletApp` kept as rollback fallback until UI-P05 |
| AI-P03 | ADR-0030 + ACP wire messages + EscrowVault + replay protection | `[S4-AI-P03]` (this commit) | 2026-11-19 | Code prompt: `strangecoin-net` `protocol/acp.rs` (4 messages `0x0E..=0x11`, secp256k1-signed canonical-JSON envelopes, feature `acp`) + `acp/{handler,nonce}` (escrow verification #24, write-ahead replay #25, `EscrowSettlement` trait with production `NoopEscrowVault` until AI-P04) + shared `strangecoin_core::escrow` calldata codec + `sc.value` host fn (deposit value-binding — pooled-drain guard) + `EscrowVault` WAT example (8 unit tests) + `tests/acp_messages.rs` (8 green: 5 КГ + 3 spec §11 extras) + fuzz target `acp_handler` (nightly matrix #11, stub-harness green) + ADR-0030 Proposed→Accepted with 14 implementation amendments + spec v1.1; found latent `collect_block_codes` off-by-one (mempool 1-based creates miskey the code registry — deploys ride a craft-path workaround in tests, recorded in the commit) |
| AI-P02 | ADR-0029 + MCP server scaffold (7 tools, 4 resources, stdio + HTTP+SSE) | `[S4-AI-P02]` (this commit) | 2026-11-18 | Code prompt: `crates/strangecoin-mcp/` (McpServer JSON-RPC dispatch + Ed25519 signing, 7 tools, 4 resources, stdio + Noise-XX-over-HTTP SSE transports) + root wiring (feature `mcp` default-on, `impl McpBackend for BlockchainFacade`, `--mcp-stdio` + `--mcp-data-dir`, `[mcp]` config, in-process HTTP listener) + `tests/mcp_server.rs` (8 green: 4 КГ + 4 spec extras) + fuzz target `mcp_determinism` (nightly matrix #10) + ADR-0029 Proposed→Accepted with 12 implementation amendments + spec §9/§11/§12; `mcp-cli` round-trip env-gated (`STRANGECOIN_MCP_CLI_TEST=1`, documented setup spec §9.1) |
| AI-P01 | ADR-0028: Strangecoin as cryptocurrency for AI agents (vision + account model) | `[S4-AI-P01]` (this commit) | 2026-11-16 | Design-only: ADR-0028 + `docs/spec/agent_integration.md` (+ 3 sibling specs relocated `analytics/` → `docs/spec/` — path correction, content unchanged); bip32 pin decision fixed (version resolves AI-P05); ADR content originally landed in `689ccbc analytics` (non-conforming message — deviation recorded in commit body) |
| UI-P01 | ADR-0031: Professional UI replacement (render backend + design system principles) | `[S4-UI-P01]` | 2026-11-16 | Design-only: ADR-0031 Status Proposed → Accepted + `docs/spec/ui_professional.md` (уже существовали с `689ccbc`, доведены `a3fead3` — deviation recorded in commit body); pin policy eframe/egui `=0.22.0` + fluent зафиксированы в AGENTS.md §Stage 4 Pin Policy (Cargo.toml не трогается — design-only, прецедент AI-P01); unblocks UI-P02 |
| UI-P02 | Design system + 14 components + tokens + themes + i18n + a11y audit (`crates/strangecoin-ui/`) | `[S4-UI-P02]` (initial `17d1ebe`, completion this commit) | 2026-11-26 | Foundation: `crates/strangecoin-ui/` workspace member, `UiBackend`/`EframeEguiBackend`, 7×11 color shades + 8 spacing + 7 typography + 5 radius + 4 shadow, 14 components × light/dark, 134 locale strings × en/ru/zh. Completion closes the real gaps: deterministic pure-Rust WCAG AA gate (`accessibility.rs` + `focus.rs`: contrast, 2.4.7 focus, 2.5.5 44×44, labels) replacing a no-op curl/node harness; vendored axe 4.9.1 as opt-in `a11y-axe` (advisory); 28 insta style snapshots; 5 per-screen audits; real `[ui] theme/locale` config + manifest-relative fluent resources; strict UI clippy + explicit CI a11y step. Deviations recorded in ADR-0031 Amendment B |

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
| Baseline RUSTSEC ignores removal (10+ IDs, 2026-11-09) | 4+ | ACCEPTED baseline in `deny.toml` + 12 `--ignore` in `nightly.yml`; cargo-audit does not read deny.toml. Each ignore carries date/reason; fixable ones (bytes/tokio/rand/zstd) need a dependency-update prompt, unmaintained/unsound ones need an upstream release. **GITHUB-P35 added NO new ignores** | dependency-update prompt per crate (no SCIP — dev/dependency layer) |
| Full-workspace / full-core mutation score | 4+ | The nightly signal is bounded to the 15-mutant fork-choice file (`cargo mutants -p strangecoin-core -f crates/strangecoin-core/src/chain_selector.rs --in-place`): `--workspace` timed out in run #20 (6387 mutants, 875s baseline build + 190s test) and even `-p strangecoin-core` timed out in the first fix run (1175 mutants, ~80s each). MISSED mutants stay signal-only | dedicated mutation-budget prompt / larger runner |

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
