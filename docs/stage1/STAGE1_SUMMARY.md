# Stage 1 DoD Verification (S1-P22)

**Document**: `docs/stage1/STAGE1_SUMMARY.md`
**Status**: Complete ✅
**Date**: 2026-10-04
**Prompt**: S1-P22 — Definition of Done verification for Stage 1
**Gate**: Tag `v1.1.0-stage1` (Gate Stage 1.5)
**Predecessor**: Tag `v1.0.0-stage0` (D03)

---

## 1. DoD Criteria — 15 items (ROADMAP3 Stage 1 / prompt-stage1 §4)

| # | Criterion | Evidence | Status | Residual | Predecessor verified |
|---|-----------|----------|--------|----------|----------------------|
| 1 | strangecoin-core created; pure functions moved; 0 I/O | `crates/strangecoin-core/` modules: serialize, consensus, state, economics, governance, address, chain_selector, vm. `rg "std::fs\|std::net\|tokio\|leveldb" crates/strangecoin-core/src` → **0 matches**. Caveat BUG-S0-013 (`verify_block_stateless` post-root) — **closed 2026-10-11 (BUG-S1-003)**: stateless verifier now recomputes and checks the post-state root | ✅ | нет (caveat closed BUG-S1-003, 2026-10-11) | да |
| 2 | State commitment + `state.root_after == block.state_root` | `crates/strangecoin-core/src/state/sparse_merkle.rs` (binary SMT depth 256; S1-P06 claimed Verkle — **amended 2026-10-08**, BUG-S0-011); `src/blockchain/block_executor.rs` — zero-root reject + compare с computed root (SCIP-0002, BUG-S1-002 closed 2026-10-11); mine/grant коммитят реальный корень; `INVARIANTS_ENFORCED.md` №19 = ✅; tests: core `tests/state_root.rs` (9), e2e `tests/state_root.rs` (6) | 🟡 residual §6.1+§6.7 | §6.1 offline genesis key (ops gate via SCIP-0001, burned testnet key); §6.7 SMT not Verkle (BUG-S0-011, true Verkle deferred Stage 3+) | да |
| 3 | Headers-first sync works | `tests/sync_headers.rs` (3 real-TCP tests: fresh sync 20 blocks, equal chain, longer fork) | ✅ | нет | да |
| 4 | Events bus works (3 subscribers) | `tests/events.rs::three_subscribers_each_receive_live_node_events` | ✅ | нет | да |
| 5 | Tie-breaking deterministic (proptest) | `tests/chain_selector_proptest.rs` (7 tests: work→timestamp→hash, permutation invariance, transitivity) | ✅ | нет | да |
| 6 | tokio introduced; new subsystems async | `Cargo.toml` tokio deps; `docs/ADR/0007-tokio-on-stage-1.md`; `tests/two_clients.rs::node_runs_on_tokio_and_shuts_down_cleanly` | ✅ | нет | да |
| 7 | network_id in HELLO; foreign peers rejected | `tests/network_id.rs::foreign_network_id_is_rejected_and_banned` | ✅ | нет | да |
| 8 | bech32 round-trip | `crates/strangecoin-core/src/address.rs::tests::test_round_trip`; HRP sc1/tsc1/rsc1; `tests/two_clients.rs::bech32_address_transfer` | ✅ | нет | да |
| 9 | Blockchain decomposed (4 + consensus_manager) | `src/blockchain/`: `chain_selector.rs`, `block_executor.rs`, `state_cache.rs`, `blockchain_facade.rs`, `consensus_manager.rs`. Line-count (2026-10-08 residual-trim): facade 361, block_executor 571, state_cache 586, chain_selector 285 (serde/wire snapshot included), consensus_manager 132 | ✅ | нет | да |
| 10 | Sync engine breaks network↔blockchain cycle | `src/network/sync_engine.rs` (ADR-0010); rg audit: `adopt_candidate`/`apply_tx`/`save_state` only in `sync_engine.rs`; `tests/sync_engine.rs` | ✅ | нет | да |
| 11 | consensus_version + activation height | `crates/strangecoin-core/src/governance/scip.rs`; `tests/consensus_version.rs` (3); core scip activation tests (5) | 🟡 residual §6.3 | §6.3 TLA+ bounded model PASS (TLC 2.19, 2026-10-09); unbounded model / reorg / state-commitment trie не смоделированы | да |
| 12 | All Stage 0 invariants still enforced | `docs/stage1/INVARIANTS_ENFORCED.md` (S1-P20); full workspace suite green | ✅ (closed 2026-10-11 BUG-S1-002) | INVARIANTS_ENFORCED.md: №22 = ✅ (closed BUG-S1-001, 2026-10-10: release run 38071514479 green + evidence); №19 = ✅ (closed BUG-S1-002, 2026-10-11: SCIP-0002 zero-root opt-out removed); residual count 0; deferrals 15/18/20 unchanged | да |
| 13 | ADR-0006..0010 written | `docs/ADR/0006-verkle-trie-vs-smt.md`, `0007-tokio-on-stage-1.md`, `0008-rocksdb-plan.md`, `0009-events-bus.md`, `0010-sync-engine.md` | ✅ (closed 2026-10-10 BUG-S1-001) | practical verification done: first `v*` release run green (tag `v0.0.1-rc1`, run 38071514479) — reproducible-build evidence in `docs/security/REPRODUCIBLE_BUILDS.md`; invariant №22 ✅ | да |
| 14 | Changelog: «1.1.0 — core extracted + Verkle + headers-first» | `Changelog.md` section `## 1.1.0 — Stage 1` | ✅ (this commit) | нет | да |
| 15 | Tag `v1.1.0-stage1` | Annotated tag placed after this commit and green verification | 🟡 residual §6.2 | §6.2 job `fuzz-canonical-decode` добавлен (BUG-S0-023), но первый прогон на GitHub Actions не выполнен; outcome pending → `fuzz/README.md` | да (tag `68e358f`) |

Plus security-track DoD: first fuzz target — `fuzz/fuzz_targets/canonical_decode.rs` (S1-P19) — **🟡 residual §6.2** (job `fuzz-canonical-decode` exists; first CI run pending, BUG-S0-023).

**Residual column (BUG-S0-030, 2026-10-09):** ✅ = criterion fully met; 🟡 = verified with open §6 residual — status downgraded until the §6 item closes. The Residual cell links criterion → §6 item (да/нет + §-reference).

**Predecessor verification (BUG-S0-003, 2026-10-07):** Stage 0 was closed via debt prompts **before** the Stage 1 gate; the chain is traceable in git:

| Debt | Prompt closed | Commits | Tag |
|------|---------------|---------|-----|
| D01 | P19 (integration tests) | `9dd8077`, `1929f2f` | — |
| D02 | P22 (THREAT_MODEL + Changelog correction) | `d66829e` | — |
| D03 | P26 (Stage 0 DoD + version sync) | `60e1840`, `3dd37ef` | `v1.0.0-stage0` @ `3dd37ef` |

Gate Stage 1 satisfied: tag `v1.0.0-stage0` exists on the last D03 commit; all S1-PXX commits are descendants of it (`git merge-base --is-ancestor 3dd37ef 68e358f` → yes). All 21 delivery commits in §2 verified present in HEAD (BUG-S0-002 fixed — atomic history restored). Stage 0 did **not** complete all 26 original prompts inline: P19/P22/P26 were executed as D01–D03 on the debt track; this document and `AGENTS.md` carry that caveat explicitly.

---

## 2. What was delivered (S1-P01 → S1-P21)

| Prompt | Commit | Predecessor verified | Summary |
|--------|--------|----------------------|---------|
| S1-P01 | `cb0ac1a` | да (commit exists in HEAD) | Grant blocks gated behind `allow_grant_blocks` (regtest-only); magic-strings removed from consensus path; `GrantBlocksDisabled` typed error; `tests/grant_flag.rs` |
| S1-P02 | `d5d55af` | да (commit exists in HEAD) | Cargo workspace + `strangecoin-core`; serialize moved; ADR-0008 RocksDB plan |
| S1-P03 | `ec4749a` | да (commit exists in HEAD) | consensus rules + economics into core; proptest moved |
| S1-P04 | `458d126` | да (commit exists in HEAD) | Pure `apply_block`/`unapply_block` in `core/state.rs`; round-trip property tests |
| S1-P05 | `4c2a0ce` | да (commit exists in HEAD) | SCIP skeleton + `consensus_version` + activation height; format_version bump; `docs/SCIP/` |
| S1-P06 | `c3d609f` | да (commit exists in HEAD) | Verkle Trie + `block.state_root`; ADR-0006 |
| S1-P07 | `6d852a1` | да (commit exists in HEAD) | StateWitness + stateless verification API |
| S1-P08 | `d97e35d` | да (commit exists in HEAD) | Merkle `tx_root` in block header |
| S1-P09 | `af74952` | да (commit exists in HEAD) | EventBus (crossbeam) + ADR-0009 |
| S1-P10 | `463d0a3` | да (commit exists in HEAD) | tokio runtime + ADR-0007; legacy threads intact |
| S1-P11 | `7192776` | да (commit exists in HEAD) | chain_selector + deterministic tie-breaking (work→timestamp→hash) |
| S1-P12 | `9c35fe4` | да (commit exists in HEAD) | block_executor + state_cache |
| S1-P13 | `3b14946` | да (commit exists in HEAD) | blockchain_facade + consensus_manager (5-component split) |
| S1-P14 | `1522fb1` | да (commit exists in HEAD) | network_id in genesis + HELLO handshake |
| S1-P15 | `495cce3` | да (commit exists in HEAD) | bech32 addresses (HRP sc1/tsc1/rsc1) |
| S1-P16 | `945889e` | да (commit exists in HEAD) | headers-first sync (GET_HEADERS/HEADERS/GET_BLOCKS/BLOCKS) |
| S1-P17 | `f48766a` | да (commit exists in HEAD) | Mempool RBF (feerate, find_replaceable, limits) |
| S1-P18 | `4af82a3` | да (commit exists in HEAD) | SyncEngine inbox (ADR-0010); network write-path eliminated |
| S1-P19 | `3a7e15f` | да (commit exists in HEAD) | Stage 1 integration matrix + first fuzz target + soak fallback |
| S1-P20 | `a0f842d` | да (commit exists in HEAD) | 22-invariant re-audit after strangler migration |
| S1-P21 | `a3a6c43` | да (commit exists in HEAD) | THREAT_MODEL v3.0 (V-34..V-42) + INCIDENT_RESPONSE v3.0 |

S1-P22 itself: `68e358f` — annotated tag `v1.1.0-stage1` placed on this commit (Gate Stage 1.5).

**Predecessor chain (verified 2026-10-07):** all 21 delivery commits above are descendants of the Stage 0 gate tag `v1.0.0-stage0` @ `3dd37ef` (last D03 commit). Debt-track closure before the gate: D01 (P19) `9dd8077`/`1929f2f` → D02 (P22) `d66829e` → D03 (P26) `60e1840`/`3dd37ef`. Every row marked «да» was checked with `git log -1 <hash>` against HEAD; BUG-S0-002 (squashed history) is fixed — full atomic history restored.

---

## 3. Verification runs (S1-P22)

**Date**: 2026-10-04

| Command | Result |
|---------|--------|
| `cargo test --workspace` | **GREEN** — strangecoin lib 34 + 20 integration targets; strangecoin-core lib 47 + 7 test targets; all suites passed |
| `cargo clippy --all-targets -- -D warnings` | **GREEN** — exit 0 (clippy debt fixed in this prompt: unused vars/mut, dead code in verkle scaffolding, needless_range_loop, map_or→is_some_and, derivable Default, unused test imports) |
| `cargo run --example canonical_decode_soak` | **GREEN** — 474,380 inputs in 10s, 0 panics (fuzz smoke; cargo-fuzz not runnable on this Windows host — see `fuzz/README.md`) |
| `SOAK_SECONDS=600 cargo run --example canonical_decode_soak` (2026-10-09, BUG-S0-023) | **GREEN** — 25,947,107 inputs in 600s, 0 panics; coverage-guided 10-min run delegated to CI job `fuzz-canonical-decode` (cargo-fuzz 0.13.2, ubuntu-latest, push/PR) |

**rg audits**:
- `crates/strangecoin-core/src`: no `std::fs` / `std::net` / `tokio` / `leveldb` → 0 I/O confirmed.
- `src/network/`: `adopt_candidate` / `apply_tx` / `save_state` only in `sync_engine.rs` (+ doc refs) → cycle broken.
- `src/`: no `\.balances\.(insert|remove|get_mut)` outside `state_cache.rs`/`block_executor.rs` and no `\.balances =` replacement outside `state_cache.rs`/`chain_selector.rs` → 0 matches (BUG-S0-022, 2026-10-09; gate: `tests/balances_gate.rs` + CI job `source-gates`).

---

## 4. ARCHITECT3 §17 checklist — Stage 1 applicable items

| Item | Status | Note |
|------|--------|------|
| Чистое ядро: serialize/consensus/state/economics/governance — 0 I/O | ✅ | rg audit above |
| Инварианты 1-22 enforce | ✅ | `docs/stage1/INVARIANTS_ENFORCED.md` |
| Threat model: новые векторы покрыты | ✅ | THREAT_MODEL v3.0 (S1-P21) |
| Events bus: события через EventBus | ✅ | ADR-0009 + `src/events.rs` |
| Blockchain декомпозиция: 5 компонентов | ✅ | `src/blockchain/` |
| Sync engine: разрыв цикла | ✅ | ADR-0010 + rg audit |
| VM trait в core, runtime отдельно | ✅ | `crates/strangecoin-core/src/vm/traits.rs` (declaration only; wasmi — Stage 1.5) |
| Protocol messages GetHeaders/Headers | ✅ | binary tags 0x01..0x04 (S1-P16) |
| Tie-breaking: work → earliest timestamp → lowest hash | ✅ | proptest + fork_choice.md |
| Mempool RBF policy implemented | ✅ | S1-P17 + `tests/rbf.rs` |
| Governance: SCIP + activation height | ✅ | S1-P05 + `docs/SCIP/` |
| Security: TLA+ + fuzzing targets | ✅ | `docs/spec/README.md` TLC PASS 2026-10-09 (BUG-S0-026); fuzz target S1-P19 |
| Strangler pattern: subsystem → crate | ✅ | strangecoin-core |
| Tests: unit + proptest + integration | ✅ | workspace suite |
| Documentation: ADR для архитектурных решений | ✅ | ADR-0006..0010 |

**Deferred / N/A at Stage 1** (per prompt-stage1 §7): peer store/connection_pool split (Stage 2), storage schema migrations (Stage 3 — ADR-0008 plan only), wallet crate split, fee invariant (Stage 5), CI all-platforms green run (release.yml fixed D03; Actions run pending first v* tag — residual).

---

## 5. Deferred to later stages

| Item | Stage | Artifact now |
|------|-------|--------------|
| WASM VM / wasmi / gas / precompiles | 1.5 | `VmExecutor` trait only (`core/src/vm/traits.rs`) |
| Noise Protocol, Erlay, gossip opts | 2 | — |
| Network crate migration (`strangecoin-net`) | 2 | ARCHITECT3 §10.4; async migration of mining/P2P done earlier — ADR-0011 (2026-10-08, BUG-S0-021) |
| RocksDB migration | 3 | ADR-0008 plan |
| EncryptedMempool, EIP-1559, AA, MEV | 5 | fee_market.rs stub |
| PoS / Casper / BLS12-381 | 7 | consensus_manager enum readiness |
| Full TLA+ formal verification | 6 | bounded full-property model + TLC PASS (BUG-S0-026, 2026-10-09); unbounded/reorg/state-commitment — Stage 6 |

---

## 6. Open obligations (carried forward)

1. **Offline genesis key** — **updated 2026-10-08 (BUG-S0-015 / S1.5-P01)**: seed-derived `genesis_keypair()` and the public seed string `"strangecoin-genesis-seed-2026"` **removed from code**; `genesis.json` carries only `initial_holder_pubkey`; SCIP-0001 created (`docs/SCIP/scip-0001-genesis-key-replacement.md`); test `tests/genesis_key.rs`. Residual is now **operational**: before mainnet freeze/block 1, generate an offline key, write only the pubkey, update `EXPECTED_GENESIS_HASH`. Current testnet genesis key is **burned**. Reflected in §1 criterion #2 status (🟡 residual §6.1, BUG-S0-030).
2. **cargo-fuzz on Windows** — **updated 2026-10-09 (BUG-S0-023 / S1.5-P07)**: coverage-guided fuzzing still not runnable on this Windows host (no MSVC/ASan), but the capable-host requirement is now delegated to CI — job `fuzz-canonical-decode` in `.github/workflows/ci.yml` runs cargo-fuzz 0.13.2 for 600s on ubuntu-latest on every push/PR (crash → job fail + `fuzz/artifacts/` upload). Fallback soak: measured 10-minute local run 2026-10-09 clean (§3). Residual: first CI run pending until the workflow executes on GitHub Actions; record its outcome in `fuzz/README.md`. Reflected in §1 criterion #15 + security-track DoD statuses (🟡 residual §6.2, BUG-S0-030).
3. **TLA+ coverage** — **updated 2026-10-09 (BUG-S0-026 / S1.5-P08)**: bounded model (2 addresses, ≤3 blocks) with the **full safety set** — `NoDoubleSpend`/`NoInflation`/`AllTxSigned`/`NonceMonotonic`/`PowValidity`/`ChainContinuity`/`ChainIdConsistency`/`SupplyConsistency`/`TypeInvariant` + `Liveness` (WF mining) — **model-checked by TLC 2.19: PASS**, 35,207 states, complete state graph (`docs/spec/README.md`). Residual: **unbounded** model (n accounts / m blocks) not checked; spec does not model the state-commitment trie / reorg / real crypto (abstract signature, scaled emission constants — tail branch live in-model). Rust tests remain the coverage for the production implementation. Reflected in §1 criterion #11 status (🟡 residual §6.3, partial, BUG-S0-030).
4. **Release pipeline** — **closed 2026-10-10 (BUG-S1-001)**: pre-flight fixes to `release.yml` (aggregate checksums collision → subdirs; `base64-subjects` per SLSA generator contract; `RUSTFLAGS` expression expansion; retired `macos-13` → `macos-latest`; `SHA256SUMS.txt` asset) committed at `a830c51`, tagged `v0.0.1-rc1` → first GitHub Actions release run `38071514479` **green** (6 matrix builds + aggregate + sign + SLSA provenance + release, 31 assets). Independent verification: local SHA256 match (windows/linux x86_64), `cosign verify-blob` = `Verified OK`, `slsa-verifier` = `PASSED` @ commit `a830c51`. Evidence: `docs/security/REPRODUCIBLE_BUILDS.md` §Verified Release Runs. Reflected in §1 criterion #13 (✅) + criterion #12 (invariant №22 = ✅ in INVARIANTS_ENFORCED.md); THREAT_MODEL V-33 closed.
5. **Zero `state_root` opt-in** — **closed 2026-10-11 (BUG-S1-002 / SCIP-0002 + BUG-S1-003)**: unconditional opt-out удалён из `root_after` и `validate_and_apply`; mainnet/testnet отвергают блоки с `state_root == [0;32]` (config-gate запрещает opt-in вне regtest); genesis (index 0) освобождён по инварианту №19; mine/grant-пути коммитят реальный post-state root; legacy regtest-БД остаются валидными через network-aware default `Config.allow_zero_state_root` (regtest=true). Деструктивная миграция stored-блоков не выполнялась — перезапись `state_root` изменила бы block hashes (детали в SCIP-0002 §Migration). Stateless post-root gap (BUG-S0-013) закрыт тем же днём (BUG-S1-003): `verify_block_stateless` пересчитывает post-root из parent root + witness + блока (`sparse_merkle.rs::root_after_updates`, mulproof update) и сверяет с `block.state_root` → `PostStateRootMismatch`; witness обязан покрывать все touched-адреса; genesis-exempt зеркалит `root_after`. Reflected in §1 criterion #1/#2 (§6.5 residual removed) + criterion #12 (№19 = ✅); INVARIANTS_ENFORCED.md residual count → 0; THREAT_MODEL V-35 + V-36 updated.
6. **Repo hygiene** — **closed 2026-10-07** (BUG-S0-008 / BUG-S0-032): removed `test.md`, `ComputeGenesisHash/`, `compute_genesis_hash.rs`, `compute_genesis_hash_toml` from the working tree; moved `run_3_wallets.ps1` → `scripts/dev/run_3_wallets.ps1`; `.idea/`/`.codebuddy/` confirmed untracked (gitignored). No untracked repo-root artifacts remain in the git index.
7. **True Verkle / KZG commitments** — **updated 2026-10-08** (BUG-S0-011 / S1.5-P02): the S1-P06 «Verkle Trie» was a flat 256-slot Merkle (not a trie; collisions >256 accounts). Replaced by a binary Sparse Merkle Tree (depth 256 over `blake3(address)`, ADR-0006 amended). True Verkle (KZG + BLS12-381) remains deferred to Stage 3+ pending a mature I/O-free crate. **Breaking:** historical `state_root` values are invalid under the SMT — reset LevelDB / resync testnet nodes. DoD §1 criterion 2 evidence updated: implementation is SMT, not Verkle. Reflected in §1 criterion #2 status (🟡 residual §6.7, BUG-S0-030).

---

## 7. Version sync

| Location | Before | After |
|----------|--------|-------|
| root `Cargo.toml` | 1.0.0 | **1.1.0** |
| `crates/strangecoin-core/Cargo.toml` | 1.0.0 | **1.1.0** |
| `Changelog.md` | 1.0.0 section only | **1.1.0 section** |
| `AGENTS.md` | Stage 0 / v1.0.0 | **Stage 1 / v1.1.0** |
| git tag | `v1.0.0-stage0` | **`v1.1.0-stage1`** (after this commit) |

---

## 8. Process note (retro §8.1)

Artifacts (vm/traits.rs, this summary, Changelog 1.1.0, version bumps) exist **before** the S1-P22 commit and tag. The tag is the **last** action, only after green `cargo test --workspace`, green `cargo clippy --all-targets -- -D warnings`, and green fuzz smoke.
