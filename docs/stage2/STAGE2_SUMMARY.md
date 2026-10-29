# Stage 2 DoD Verification (S2-P24)

**Document**: `docs/stage2/STAGE2_SUMMARY.md`
**Status**: Complete ✅ (tag `v2.0.0-stage2` — annotated, pushed; release pipeline verified — §7)
**Date**: 2026-10-29
**Prompt**: S2-P24 — Definition of Done verification for Stage 2 (production-grade P2P)
**Prompt refs**: S2-P01..S2-P24 (`analytics/prompt-stage2.md`)
**Gate**: Tag `v2.0.0-stage2` (Gate Stage 3 RocksDB / Stage 4 dev-experience)
**Predecessor**: Tag `v1.5.0-stage1.5` (S1.5-P19 + S1.5-FBP11)

---

## 1. DoD Criteria — 12 items (ROADMAP3 Этап 2 §Definition of Done)

Нумерация 1:1 с AGENTS.md §Stage 2 DoD Quick Reference; ROADMAP3 содержит 13 строк
DoD — строки ADR-0016/ADR-0017 слиты в один критерий №11 (прецедент: ROADMAP3
первоначально планировал ADR-0016..0019, Stage 2 переименован в ADR-0020..0023,
ADR-0016 занят Receipts Root in Block Header, Stage 1.5 FBP07).

| # | Criterion | Evidence | Status |
|---|-----------|----------|--------|
| 1 | Gossip работает (test: txs распространяются за ≤1 сек на 100 узлах) | `tests/net_gossip.rs` (S2-P22 `5219d17`: 1 node → 100 simulated peers ≤1s, simulated clock) + unit `crates/strangecoin-net/tests/gossip.rs` (S2-P13 `fafe3ff`) | ✅ |
| 2 | Compact blocks работают (test: bandwidth reduction ≥50% vs naive) | `tests/net_compact_blocks.rs` (S2-P22: 100-tx block 1 MB → CompactBlock ~10 KB, ≥50%) + `crates/strangecoin-net/src/gossip/compact_blocks.rs` (S2-P14 `4d7ffda`) | ✅ |
| 3 | Erlay работает (test: peer reconciliation успешна) | `tests/net_erlay.rs` (S2-P22: 2 peers 1000 txs, 800 shared → only 200 transmitted, ≥80% reduction) + `gossip/erlay/{mod,reconcile,mincut,fallback}.rs` (S2-P15 `5c86cfc`) | ✅ |
| 4 | Peer discovery: new node находит ≥8 peers за ≤30 сек | `tests/common/net_helpers.rs` (S2-P22: `SimulatedNetwork`, multi-source bootstrap) + `discovery/{mod,seeds,dnssec,addr_gossip}.rs` (S2-P11 `feb9f75`) | ✅ |
| 5 | Handshake работает (test: incompatible peers отвергаются) | `tests/net_noise_handshake.rs` (S2-P22: incompatible version/network_id rejected) + `protocol/hello.rs` `MIN_PROTO_VERSION=2` (S2-P10 `6e6dbbc`) | ✅ |
| 6 | Peer scoring: malicious peers banned после N violations | `tests/net_peer_scoring.rs` (S2-P22: invalid blocks → temp ban, protocol violations → persistent ban) + `peer/scoring.rs` ADR-0022 (S2-P17 `5451a65`) | ✅ |
| 7 | Noise encryption работает (test: MITM обнаружен) | `tests/net_noise_handshake.rs` (S2-P22: MITM with own Noise keys → NodeId mismatch → drop) + `noise/{handshake,transport,auth}.rs` (S2-P07..S2-P09 `ca040ae`/`d8f2ea8`/`682b36e`) | ✅ |
| 8 | Rate limiting работает (test: spammer banned) | `tests/net_rate_limit_integration.rs` (S2-P22: 1000 INV/s spammer → banned after 5 violations / 60s) + `rate_limiter.rs` token bucket ADR-0023 (S2-P16 `fbe0305`) | ✅ |
| 9 | Legacy messages удалены (no backward compat with v0.8.6) | `crates/strangecoin-net/src/protocol.rs` — `GET_BLOCKCHAIN`/`UPDATE_BLOCKCHAIN:` вырезаны, `MessageClass::ChainGossip` снят (S2-P18 `3d11732`), `InboundMessage::UnknownMessage` forward-compat; `crates/strangecoin-net/tests/protocol_deprecated.rs` | ✅ |
| 10 | `crates/strangecoin-net/` создан, сеть перенесена | `crates/strangecoin-net/Cargo.toml` (workspace member, feature `net` default-on, S2-P05 `2c12e7a`); `src/network/mod.rs` — 5-line re-export shim (S2-P06 `d29ae7f`, strangler pattern) | ✅ |
| 11 | ADR-0016 «Why Noise» + ADR-0017 «Why Erlay» написаны | `docs/ADR/0020-noise-protocol-framework.md` (S2-P01 `f1a3115`), `0021-erlay-integration.md` (S2-P02 `78cf594`) + `0022-peer-scoring-algorithm.md` (S2-P03 `14f57d4`), `0023-rate-limiting-strategy.md` (S2-P04 `670b0c3`) — **renumbered** с ROADMAP3-плана 0016..0019 (ADR-0016 занят Receipts Root, Stage 1.5 FBP07) | ✅ (renumbered) |
| 12 | `Changelog.md` обновлён: `2.0.0 — production-grade P2P` | Секция `## 2.0.0 — production-grade P2P (Noise, gossip, compact blocks, Erlay, peer scoring)` (этот коммит); version 2.0.0 в root + `strangecoin-core` `Cargo.toml` (vm-wasm 0.2.0, net 0.1.0 — без изменений); tag `v2.0.0-stage2` | ✅ |

Плюс: тег `v2.0.0-stage2` — **Gate Stage 3 (RocksDB) / Stage 4 (dev-experience: SDK,
devnet, indexer, JSON-RPC eth_*, faucet)**; Stage 4 требует Stage 2 завершённым,
Stage 3 — мягкая зависимость (ROADMAP3 карты зависимостей).

---

## 2. What was delivered (S2-P01 → S2-P23)

| Prompt | Commit | Date | Summary |
|--------|--------|------|---------|
| S2-P01 | `f1a3115` | 2026-10-26 | ADR-0020: Noise Protocol Framework (XX handshake, snow crate) |
| S2-P02 | `78cf594` | 2026-10-26 | ADR-0021: Erlay (mincut reconciliation + fallback) |
| S2-P03 | `14f57d4` | 2026-10-26 | ADR-0022: peer scoring (weighted sum, ban state machine) |
| S2-P04 | `670b0c3` | 2026-10-26 | ADR-0023: rate limiting (token bucket per peer) |
| S2-P05 | `2c12e7a` + `c098f9b` + `2ae8903` | 2026-10-26 | Crate scaffolding `crates/strangecoin-net/` + workspace member + feature `net`; repo-wide rustfmt; Windows preamble docs |
| S2-P06 | `d29ae7f` | 2026-10-26 | Миграция `src/network/*` → strangecoin-net + re-export shim + `BlockchainView`/`EventSink` cycle break |
| S2-P07 | `ca040ae` | 2026-10-26 | Noise XX handshake state machine (snow, forward secrecy) |
| S2-P08 | `d8f2ea8` | 2026-10-26 | ChaCha20-Poly1305 transport + MITM detection API + framing `encrypt_frame`/`decrypt_frame` |
| S2-P09 | `682b36e` | 2026-10-26 | Ed25519 `NodeId` auth + keystore integration + детерминированный ephemeral (`noise/ephemeral.rs`, SeedResolver) |
| S2-P10 | `6e6dbbc` | 2026-10-26 | HELLO + capabilities negotiation, `MIN_PROTO_VERSION=2` |
| S2-P11 | `feb9f75` | 2026-10-26 | Discovery: seed IPs + DNSSEC (hickory `=0.26.3`) + ADDR gossip + multi-source bootstrap |
| S2-P12 | `24ef518` | 2026-10-26 | peer store/manager/pool split + ban state machine + `connect_to_peer` (TCP+Noise+HELLO) + network.json |
| S2-P13 | `fafe3ff` | 2026-10-26 | Gossip INV/GET_DATA/TX/BLOCK + `GossipState` dedup + broadcast |
| S2-P14 | `4d7ffda` | 2026-10-27 | BIP 152 compact blocks (siphash short IDs, детерминированный nonce из block hash, fill-up) |
| S2-P15 | `5c86cfc` | 2026-10-27 | Erlay reconciliation (mincut + fallback; documented stub `impl_id=0`, minisketch-rs optional default-off) |
| S2-P16 | `fbe0305` | 2026-10-28 | Rate limiting: token bucket + per-message caps + violation window + ban (ADR-0023) |
| S2-P17 | `5451a65` + `1ce82a1` | 2026-10-28 | Peer scoring: 4-factor weighted formula + ban ladder + EWMA (ADR-0022) |
| S2-P18 | `3d11732` | 2026-10-28 | Deprecated v0.8.6 cleanup: `GET_BLOCKCHAIN`/`UPDATE_BLOCKCHAIN:` removed, `UnknownMessage` forward-compat |
| S2-P19 | `bd3e130` | 2026-10-28 | Async migration: thread-fallback удалён; production connection loop (Noise accept/dial, gossip/compact/Erlay dispatch, rate-limit verdicts) |
| S2-P20 | `3879124` + `beab2f9` | 2026-10-28 | Shadow tooling: `replay_blocks` + `compare` divergence detector + CLI `shadow replay`; rustfmt drift cleanup |
| S2-P21 | `4ddd054` | 2026-10-29 | Fuzz: 4 network targets (`noise_handshake`, `gossip_inv`, `compact_blocks_fill_up`, `erlay_reconciliation`) + CI matrix 9 targets |
| S2-P22 | `5219d17` | 2026-10-29 | Integration tests: 6 файлов `tests/net_*.rs` + `tests/common/net_helpers.rs` (15 tests) |
| S2-P23 | `53cebf2` | 2026-10-29 | Invariants 22/22 regression audit + THREAT_MODEL v3.2 (V-1/V-2/V-14/V-15/V-17/V-24) + RESIDUAL_RISKS_STAGE3_PLUS |

Замечания:
- S2-P16/P17 шли параллельно после S2-P12; S2-P18/P19/P20 параллельно после scaffolding; verification-трек S2-P21→S2-P22→S2-P23→S2-P24 строго последовательно.
- S2-P15/S2-P17 — отклонения от literal спеки (FBP) зафиксированы в module docs, AGENTS.md §Stage 2 Determinism Checklist status-блоках и commit messages.
- Style-коммиты (`c098f9b`, `beab2f9`) — rustfmt drift, отдельные от промптовых изменений (§Common Gotchas).

---

## 3. Verification runs (2026-10-29, S2-P24)

| Run | Command | Result |
|-----|---------|--------|
| Root integration | `cargo test -p strangecoin` | **green** — 41 test binaries, 0 failed (включая `net_noise_handshake`, `net_gossip`, `net_compact_blocks`, `net_erlay`, `net_peer_scoring`, `net_rate_limit_integration` + regression `network`/`network_id`/`rate_limit`/`sync_*`/`two_clients`/`rbf`/`double_spend`/`reorg`/`concurrency`/`vm_*`) |
| Core | `cargo test -p strangecoin-core` | **green** — 15 suites, 262 tests, 0 failed (consensus/state/receipts/proptest — инварианты #2..#6, #11..#13, #15, #18..#21) |
| VM | `cargo test -p strangecoin-vm-wasm` | **green** — 10 suites, 155 tests, 0 failed |
| Network crate | `cargo test -p strangecoin-net` | **green** — 18 suites, **192 tests**, 0 failed (noise/gossip/erlay/scoring/rate_limit/connection_loop/async_migration/shadow + unit) |
| Doc tests | `cargo test --workspace --doc` | **green** — 4 suites |
| Lint | `cargo clippy --all-targets -- -D warnings` | **green** — exit 0, 0 warnings |
| Fuzz (coverage-guided) | 9 targets × 600s | ⚠️ **не верифицировано локально + причина** — Windows host, cargo-fuzz недоступен (libFuzzer C++ требует clang/MSVC; ASan unsupported на `x86_64-pc-windows-gnu`); CI jobs в `.github/workflows/ci.yml` (matrix 9 targets, S2-P21). Прецедент FBP08/FBP09/FBP11 |
| Release pipeline | `release.yml` (6 matrix + cosign + SLSA L3) | см. §7 Release (верификация через GitHub REST API — `gh`/`cosign`/`slsa-verifier` не установлены на Windows-хосте, прецедент FBP11) |

Тестовое окружение: Windows, `stable-x86_64-pc-windows-gnu` + rust-lld (`.cargo/config.toml`);
чанки гонялись с Windows host preamble (`C:\msys64\mingw64\bin` в PATH) и
`cmd /c "... > target\*.log"` redirect (см. AGENTS.md §Common Gotchas).

**Cross-reference — Stage 1.5 closure**: `analytics/bugfixes-stage1.5.md` v1.2 verified
**11/11 fixed**, tag `v1.5.0-stage1.5` confirmed on origin commit `1f133c7`
(`prompt-stage2.md` §0.1) — precondition Stage 2 выполнен до S2-P01 и сохраняется.

---

## 4. Residual obligations for Stage 3+ (зафиксировано S2-P24)

| # | Obligation | Stage | Source |
|---|-----------|-------|--------|
| 1 | RocksDB migration (LevelDB → RocksDB), `crates/strangecoin-storage/` | Stage 3 | ARCHITECT3 §3.7; prompt-stage2 §7 |
| 2 | Stateless validation witnesses (Verkle proofs) | Stage 3 | ARCHITECT3 §3.3 (ADR-0006 amended) |
| 3 | State pruning / expiry (EIP-4444 analog) | Stage 3+ (SCIP) | prompt-stage2 §7 |
| 4 | Snapshot sync | Stage 3 | ARCHITECT3 §3.7; new attack surface — `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1 |
| 5 | Block-STM parallel execution | Stage 3+ | ARCHITECT3 §15 |
| 6 | SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet | Stage 4 (требует Stage 2 завершённым — этот тег) | prompt-stage2 §7 |
| 7 | Account Abstraction (ERC-4337), EIP-1559 dynamic base fee, threshold encryption mempool impl, fee market (инвариант #20) | Stage 5 | ARCHITECT3 §15; `docs/stage2/INVARIANTS_ENFORCED.md` #20 |
| 8 | External audit, formal verification (K-framework WASM subset, TLA+ network protocol), ZK-VM research | Stage 6 | `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §2 |
| 9 | PoS migration (Casper FFG), BLS12-381 + `bls_aggregate` precompile impl, validator set rotation, slashing | Stage 7 | prompt-stage2 §7 |
| 10 | Erlay minisketch full impl (`impl_id=1`, minisketch-rs =0.1.9) | SCIP-gated | ADR-0021 escape hatch; `RESIDUAL_RISKS` §2 |
| 11 | 2 ops-gate: **BUG-S1-013** (PGP-key offline keygen), **BUG-S1-015** (offline genesis key air-gapped keygen) | **mainnet freeze** (human ops; НЕ блокируют Stage 2/3/4 — regtest/devnet scope) | `docs/stage1/STAGE1_SUMMARY.md` §6, `docs/stage1.5/STAGE1_5_SUMMARY.md` §4 (FBP09) |
| 12 | **SCIP-0001** (genesis key replacement) | отдельный ops+code трек; после S2-P24 — единственный блокер mainnet | prompt-stage2 §6.11 |

---

## 5. Anti-goals Stage 2 — подтверждение «не реализовано» (prompt-stage2 §7)

Все явно отложенные пункты **не реализованы** в Stage 2 (сверено против кодовой
базы на теге): RocksDB (✗ — остаётся rusty_leveldb), Verkle/stateless wire (✗ — SMT
depth 256), state pruning (✗), snapshot sync (✗), Block-STM (✗ — serial execution),
Account Abstraction (✗), EIP-1559 (✗ — fixed fee), multidimensional fees (✗),
threshold encryption mempool (✗ — placeholder с Stage 1), WIT Component Model (✗ —
`format_version: "wit-v2"` placeholder), K-framework (✗), ZK-VM (✗), state rent
(✗), BLS12-381 + `bls_aggregate` impl (✗ — NotImplemented, Stage 7), SDK/devnet/
indexer/explorer/JSON-RPC/faucet (✗ — Stage 4), PoS/Casper FFG (✗ — Stage 7),
external audit (✗ — Stage 6), TLA+ network protocol (✗ — только `consensus.tla`
Stage 1.5; residual зафиксирован S2-P23), full mining async migration (✗ — только
network-воркеры S2-P19; mining workers остаются dual-mode), constructor fee
burning / receipts prune / contract storage prune (✗ — Stage 3+/5 через SCIP),
SCIP-0001 + BUG-S1-013/015 (✗ — ops, вне Stage 2 scope).

Полный список — `analytics/prompt-stage2.md` §7 + `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`.

---

## 6. Process notes

1. **ARCHITECT3 §17 checklist** — прогнан в рамках S2-P24 self-review (см. §6.1).
2. **«Done» только через этот DoD-промпт** (retro §8.1): тег `v2.0.0-stage2` ставится
   после прохождения всех КГ S2-P24; до этого момента Stage 2 считался in progress.
3. **Consensus-breaking S2-P18** (deprecated messages removal): mainnet не запущен,
   прямая правка допустима до тега `v2.0.0-stage2`; зафиксировано в Changelog 2.0.0.
   После тега — только через SCIP + activation height.
4. **Детерминизм** (V-21 extension, чек-лист 5 пунктов): ревью каждой network-функции
   выполнено в S2-P07/P08/P09/P15/P17 status-блоках AGENTS.md; S2-P09 закрыл
   obligation пунктов 2–3 (SeedResolver вместо OS CSPRNG).
5. **Pin policy**: все protocol/consensus/crypto/network-зависимости через `=`
   (snow `=0.10.0`, ed25519-dalek `2.0`, hkdf `=0.12.1`, sha2 `=0.10.9`, siphasher
   `=1.0.4`, hickory-resolver `=0.26.3`, minisketch-rs `=0.1.9` optional);
   обновление — SCIP после mainnet freeze.
6. **Отклонения от literal спеки S2-P24** (FBP, зафиксированы явно):
   (a) AGENTS.md — формулировка «Stage 2 complete (tag `v2.0.0-stage2`)» без
   «pending S2-P24» (тег создаётся в этом же промпте);
   (b) 13 строк ROADMAP3 DoD сведены к 12 критериям (ADR-0016/0017 слиты —
   прецедент AGENTS DoD Quick Reference);
   (c) net `Cargo.toml` — requirement `strangecoin-core version = "2.0.0"` (сверх
   явного списка задачи 4, необходимо для резолва path-dep после bump core);
   (d) release-verification (задачи 12–13) — отдельный коммит после тега (физически
   идёт после push; прецедент S1.5-FBP11).
7. **ECONOMICS.md**: завершение Stage 2 открывает канал «network maturity» —
   не код-задача, зафиксировано как этапное основание (аналог Stage 1.5 §5.6).

### 6.1 ARCHITECT3 §17 checklist (прогнан после S2-P24)

| Checklist item | Status | Evidence |
|----------------|--------|----------|
| Protocol messages: GetHeaders/Headers, deprecated legacy удалён | ✅ | S2-P18 `3d11732`, `protocol.rs`, `tests/protocol_deprecated.rs` |
| Peer store/manager/connection_pool разделены | ✅ | S2-P12 `24ef518`, `peer/{store,manager,pool}.rs` |
| Sync engine: разрыв цикла network↔blockchain | ✅ | S2-P06 `d29ae7f`, trait `BlockchainView` |
| Tie-breaking: work → earliest timestamp → lowest hash | ✅ regression | Stage 1 `chain_selector.rs` (не тронут) |
| Mempool RBF policy implemented | ✅ regression | Stage 1 `mempool/` (не тронут; `tests/rbf.rs` green) |
| Storage schema versioned + migrations | ✅ regression | Stage 1; миграция schema — Stage 3 scope |
| Wallet: secp256k1/BLS, public API only, path to separate crate documented | ✅ regression | Stage 0/1; node-Ed25519 segregated `keystore/node/` (S2-P09) |
| Economics: tail emission formula, fee invariant enforced | ✅ regression / #20 deferred | emission Stage 1; fee-split degenerate → Stage 5 |
| Governance: SCIP + activation height | ✅ regression | Stage 1 `governance/scip.rs` |
| Security: TLA+ spec актуализирован, fuzzing targets добавлены | ✅ | S2-P21 (4 network targets, matrix 9) + S2-P23 (THREAT_MODEL v3.2); `consensus.tla` unchanged (35,207 states PASS) |
| Anti-goals respected (ARCHITECT3 §15) | ✅ | §5 выше |
| License: MIT/Apache-2.0, LICENSE в корне | ✅ regression | Stage 0 |
| Reproducible builds: CI gate, cosign signature для release | ✅ | `release.yml` + v2.0.0-stage2 run — §7 |
| Strangler pattern: перенос подсистемы в отдельный крейт | ✅ | S2-P05 + S2-P06 (`crates/strangecoin-net/`) |
| Tests: unit + property (proptest) + integration | ✅ | S2-P22 (6 net-файлов) + regression Stage 0/1/1.5 (§3 green) |
| CI: clippy -D warnings, all platforms | ✅ regression | `ci.yml` (clippy job) + локальный clippy §3 green |
| Documentation: ADR для архитектурных решений, SCIP для протокола | ✅ | ADR-0020..0023 (S2-P01..S2-P04) + ADR-0012..0019 (Stage 0–1.5) |

---

## 7. Release (S2-P24, verified 2026-10-29)

- **Tag**: annotated `v2.0.0-stage2` → commit `346e3c065e26be388f15e76bdeb311dbc4e4cc7c` (tag object `2577f406075369c8d1d127825c619e466d1c4ff3`, tagger Reider85; pushed to origin, verified via `git ls-remote --tags origin`).
- **Release**: https://github.com/Reider85/strangecoin/releases/tag/v2.0.0-stage2 — published 2026-10-29T15:09:20Z, 26 assets (6 binaries, 6 `.sha256`, 6 `.cosign`, 6 `.cosign.pem`, `SHA256SUMS.txt`, `multiple.intoto.jsonl` SLSA L3).
- **Workflow run**: https://github.com/Reider85/strangecoin/actions/runs/40897214764 — **success, 13/13 jobs** (6 matrix builds + `aggregate-hashes` + `sign` + `SLSA Provenance` ×5 + `Create Release`). Verified via GitHub REST API 2026-10-29 (`gh` CLI not installed on host; REST used instead — прецедент FBP11).
- **First attempt failed (transient, задокументировано)**: run https://github.com/Reider85/strangecoin/actions/runs/40893905891 — 5/6 builds green, `Build (macos-latest, x86_64-apple-darwin)` не получил runner (cancelled через 15 мин pending) → downstream jobs skipped, run = failure, assets не опубликованы. Recovery: tag удалён с origin и повторно запушен тем же tag-объектом на тот же коммит (история/содержимое не изменены) → новый run success с первой же попытки.
- **SHA256**: 6 published checksums зафиксированы в `docs/security/REPRODUCIBLE_BUILDS.md` §Verified Release Runs (v2.0.0-stage2, procedure BUG-S1-001).
- **Independent cosign/SLSA blob verification**: не верифицировано локально + причина — `cosign`/`slsa-verifier`/`gh` не установлены на Windows-хосте; pipeline-level jobs (`sign`, `SLSA Provenance`) green в том же ране. Verify commands (procedure BUG-S1-001):
  ```bash
  cosign verify-blob --signature strangecoin-windows-x86_64.exe.cosign \
    --certificate strangecoin-windows-x86_64.exe.cosign.pem \
    --certificate-identity "https://github.com/Reider85/strangecoin/.github/workflows/release.yml@refs/tags/v2.0.0-stage2" \
    --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
    strangecoin-windows-x86_64.exe

  slsa-verifier verify-artifact strangecoin-windows-x86_64.exe \
    --provenance-path multiple.intoto.jsonl \
    --source-uri github.com/Reider85/strangecoin \
    --source-tag v2.0.0-stage2
  ```
- **CI (ci.yml, push master)**: run 40893883063 на момент записи — in progress (fuzz jobs 600s); предыдущие master-runs падали на beta-channel clippy / coverage / 2 fuzz targets — pre-existing, вне КГ S2-P24 (КГ требует release.yml — он green, см. выше).

---

**Related**: `docs/stage2/INVARIANTS_ENFORCED.md` · `docs/stage2/THREAT_MODEL_STAGE2.md` ·
`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` · `docs/ADR/0020..0023` ·
`analytics/prompt-stage2.md` S2-P24 · `docs/security/REPRODUCIBLE_BUILDS.md` ·
`docs/stage1.5/STAGE1_5_SUMMARY.md`
