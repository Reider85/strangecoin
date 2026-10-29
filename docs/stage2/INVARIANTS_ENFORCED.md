# Stage 2 Invariants Enforced (S2-P23 regression audit)

**Document**: `docs/stage2/INVARIANTS_ENFORCED.md`
**Status**: Complete ✅
**Date**: 2026-10-29 (S2-P23)
**Prompt**: S2-P23 — Regression 22 инвариантов + threat model update после Stage 2 proper work (S2-P01..S2-P22)
**Supersedes (for Stage 2 status)**: `docs/stage1.5/INVARIANTS_ENFORCED.md` (22/22 на теге `v1.5.0-stage1.5`, re-audit S1.5-FBP08)

**Нумерация**: 1:1 с `analytics/ARCHITECT3.md` §5 (22 инварианта; эталон — строки 484-507).
**Stage 0 baseline**: `docs/stage0/INVARIANTS_ENFORCED.md`.
**Stage 1 baseline**: `docs/stage1/INVARIANTS_ENFORCED.md`.
**Stage 1.5 baseline**: `docs/stage1.5/INVARIANTS_ENFORCED.md` (полные enforcement-локации #4/#18/#19 и промежуточные extension'ы — там же; в этой таблице ячейки Stage 1.5 для самых длинных строк сжаты с pointer'ом).

## Status Summary

| Status | Count | Invariants |
|--------|-------|------------|
| ✅ Enforced | 22 | 1-22 |
| 🟡 Residual (enforced with known gaps) | 0 | — |

**S2-P23 regression audit (2026-10-29): 22/22 enforced — no regressions**
после Stage 2 proper work (S2-P05..S2-P22: crate-миграция network в
`crates/strangecoin-net/`, Noise XX + ChaCha20-Poly1305 + Ed25519 auth,
HELLO capabilities, peer discovery + DNSSEC + ADDR gossip, peer store/manager/
pool split, gossip INV/GET_DATA, compact blocks BIP 152, Erlay reconciliation,
token bucket rate limiting, peer scoring, deprecated-сообщения cleanup, async
migration, shadow tooling, fuzz targets S2-P21, integration tests S2-P22).

**Расширение coverage (Stage 2):**
- **#16** (P2P framing) — extension: Noise encrypted framing (`encrypt_frame`/`decrypt_frame`, `MAX_MESSAGE_SIZE` check до аллокации) — S2-P08
- **#17** (Rate limiting) — extension: token bucket per peer + per-message caps + ban threshold + интеграция с peer scoring (`apply_rate_verdict` → `record_rate_violation` → ban) — S2-P16/S2-P17

**Остальные 20** — regression green (места enforcement не ослаблены; часть
локаций переехала в `crates/strangecoin-net/` вслед за S2-P06-миграцией —
см. таблицу). **#20** — по-прежнему enforced в degenerate-форме (fee=0);
настоящий fee market — Stage 5 (SCIP до mainnet freeze).

**Итог Stage 2**: 22/22 enforced (2 extensions: #16, #17; 1 still deferred до Stage 5: #20 degenerate).

## Invariant Audit Table

| # | Invariant (ARCHITECT3 §5) | Stage 0 location | Stage 1 location | Stage 1.5 location | Stage 2 location | Test(s) | Status |
|---|---------------------------|------------------|------------------|--------------------|------------------|---------|--------|
| 1 | Вся валидность — из цепочки; `balances` — кэш | `validate_chain` в `src/main.rs` | `src/blockchain/state_cache.rs::validate_chain` + `rebuild_from_chain` | без изменений (+ contract_storage/receipts как derived) | без изменений (blockchain-слой Stage 2 не тронут) | `tests/state_cache.rs`, `tests/reorg.rs` | ✅ regression |
| 2 | Каждая транзакция подписана, `sender == pubkey` (secp256k1) | `verify_transaction` в `src/main.rs` | core `consensus::verify_transaction` (`consensus.rs:343`) | без изменений | без изменений | core `tests/consensus_proptest.rs`; `tests/block_executor.rs::rejects_unsigned_transfer`; `tests/two_clients.rs` | ✅ regression |
| 3 | `hash <= target` для каждого блока при его difficulty | `validate_difficulty` (U256) | core `consensus::validate_difficulty` + `block_executor.rs::validate_target` | без изменений | без изменений | `tests/pow.rs`, `tests/block_executor.rs::rejects_proof_of_work_above_target`, `tests/sync_headers.rs`, core `tests/u256.rs` | ✅ regression |
| 4 | `apply_block`/`unapply_block` — обратные чистые функции | Implicit in `validate_chain` | core `state/inner.rs::apply_block:54` / `unapply_block:136` | extension: contract_storage + value_transfers_undo + constructor undo (полная деталь — `docs/stage1.5/INVARIANTS_ENFORCED.md` #4) | без изменений (VM/state-слой не тронут Stage 2) | core `tests/state_roundtrip.rs` (4 proptest); core `tests/contract_storage_smt.rs`; vm-wasm `tests/contract_storage.rs`; e2e `tests/vm_transfer_to_contract.rs`, `tests/vm_deploy_call.rs` | ✅ regression |
| 5 | Хэш/подпись — на канонических байтах (`serialize`), не JSON | canonical binary, blake3; 19 golden vectors | core `serialize.rs` (FORMAT_VERSION) | extension: Receipt/Create/Call per-variant; header v7 (`receipts_root`) | без изменений (core не тронут; wire-кодеки net используют собственные байтовые форматы, не consensus serialization) | core `tests/serialize_golden.rs` (19/19); proptest roundtrip; core `tests/receipts.rs` | ✅ regression |
| 6 | Блок не содержит непроверимых транзакций/наград сверх эмиссии | `validate_chain`: coinbase ≤ reward | core `state/inner.rs` + grant-gate | без изменений | без изменений | `tests/emission.rs`, `tests/grant_flag.rs`, `tests/block_executor.rs`, `tests/chain_id.rs` | ✅ regression |
| 7 | Размеры сообщений/блоков всегда ограничены до аллокации | `MAX_MESSAGE_SIZE`/`MAX_BLOCK_SIZE`/`MAX_TX_SIZE` | те же лимиты | без изменений wire-лимитов | лимиты переехали с S2-P06: `crates/strangecoin-net/src/protocol.rs::MAX_MESSAGE_SIZE:25` (32 MiB, check до аллокации); block/tx caps — в `block_executor.rs`/`state_cache.rs` (без изменений) | net framing tests; `tests/network.rs`; `tests/block_executor.rs` (5MB block / 300KB tx reject) | ✅ regression |
| 8 | Генезис детерминирован и совпадает у всех узлов | Per-network genesis ↔ expected hash | core `consensus.rs::EXPECTED_*_GENESIS_HASH`; `src/consensus/mod.rs::validate_genesis` | extension: receipts_root=[0;32] в genesis, hashes пересчитаны | без изменений | `tests/genesis_key.rs` (9), `tests/block_executor.rs::genesis_is_applied_to_an_empty_chain` | ✅ regression |
| 9 | Секреты не пишутся на диск и не логируются | keystore AES-256-GCM + PBKDF2 | `src/wallet.rs`, `src/config.rs` — нет секретов | без изменений; VM без I/O | extension (без ослабления): Ed25519 long-term node keypair в `keystore/node/` (S2-P09) — отдельно от wallet-keystore; ephemeral Noise-ключи выводятся из long-term seed, не логируются (`noise/ephemeral.rs`) | `tests/genesis_key.rs::consensus_source_has_no_genesis_seed`; `tests/net_noise_handshake.rs` (auth); code audit | ✅ regression + ext |
| 10 | **Replay protection:** каждая tx содержит `chain_id`; mainnet/testnet/regtest несовместимы | `chain_id` в verify + mempool + block validation | core verify + mempool + block_executor; HELLO `network_id` | без изменений | расширение места: HELLO-валидация переехала в `crates/strangecoin-net/src/protocol/hello.rs::validate_hello:148` (network_id mismatch → drop, `MIN_PROTO_VERSION=2`); Noise transport nonce anti-replay (`noise/transport.rs::NoiseError::Replay`, S2-P08) — второй слой поверх chain_id | `tests/network_id.rs::foreign_network_id_is_rejected_and_banned`; `tests/chain_id.rs` (5); core proptest `chain_id_validation`; `tests/net_noise_handshake.rs` | ✅ regression + ext |
| 11 | **Nonce:** `account.nonce` строго инкрементируется; tx с `nonce <= account.nonce` отвергается | `validate_chain` nonce check; mempool | core `validate_nonce`; `state/inner.rs`; `mempool/mod.rs` | extension: constructor nonce++ | без изменений | proptest `nonce_reject` (core); `tests/double_spend.rs`; `tests/two_clients.rs`; `tests/vm_deploy_call.rs` | ✅ regression |
| 12 | **Transaction hash = commitment:** `txid` из канонических байт всей tx | `txid` from signed bytes | core `serialize.rs::txid:26` | extension: per-variant serialization, receipts key = txid | без изменений | core golden-vector tests; proptest roundtrip; core `tests/receipts.rs` | ✅ regression |
| 13 | **Mempool basic rules:** на `insert` — подпись, dup, nonce, chain_id, balance | `mempool/insert()` validation | `src/mempool/mod.rs::insert:80` | без изменений | без изменений; gossip-announced txs идут тот же insert перед relay (`gossip/broadcast.rs` → mempool) | `tests/rbf.rs` (5), `tests/two_clients.rs`; `tests/net_gossip.rs` (relay path) | ✅ regression |
| 14 | **Graceful shutdown:** `Drop` для storage/wallet/network; нет «ручного удаления LOCK» | `Drop` for Node/Wallet/Storage | `Drop` for `Node` (`src/lib.rs:526`) | без изменений; tokio graceful | расширение места: network workers — `tokio::spawn` (S2-P19, thread-fallback удалён); `Drop for Node` остаётся (`src/lib.rs`, Node держит peer_manager/gossip_state/reconciliation_state); connection-loop задачи гасятся через tokio runtime shutdown | `tests/concurrency.rs::deadlock_test_blockchain_wallet_lock_order`; `tests/two_clients.rs::node_runs_on_tokio_and_shuts_down_cleanly` | ✅ regression |
| 15 | **Block gas limit:** `gas_used <= block_gas_limit` | deferred Stage 1.5 | deferred Stage 1.5 | core `vm/gas.rs::BLOCK_GAS_LIMIT` = 1M; `block_executor.rs::enforce_block_gas_limit`/`enforce_receipt_gas_limit` (pre+post) | без изменений (VM/gas не тронут Stage 2) | `tests/vm_gas_exhaustion.rs` (3); `tests/vm_deploy_call.rs`; block_executor unit; vm-wasm `tests/gas_metering.rs` | ✅ regression |
| 16 | **P2P framing:** length-prefixed, проверка размера ДО аллокации | `network/protocol.rs`: read length, validate, then alloc | `src/network/protocol.rs::read_length_prefixed:250` / async `:289` | без изменений | **extension (S2-P08): Noise encrypted framing** — `crates/strangecoin-net/src/protocol.rs::encrypt_frame:427` / `decrypt_frame:471` (length-prefix уже внутри AEAD-кадра); `MAX_MESSAGE_SIZE` check **ДО** аллокации plaintext-буфера сохранён; monotonic nonces + `NoiseError::Replay` (`noise/transport.rs`); chunking >65519 B. Plaintext-путь удалён единым Noise-транспортом (S2-P19) | net framing unit tests; `tests/net_noise_handshake.rs` (roundtrip + MITM drop); fuzz `noise_handshake.rs` (S2-P21) | ✅ Stage 2 ext |
| 17 | **Rate limiting:** лимит сообщений/сек от одного пира и per-message; нарушение → бан | `RateLimiter` sliding window | `rate_limiter.rs::check:111`, `check_class:126`, `ban:180` | без изменений (Stage 2 — расширение) | **extension (S2-P16/S2-P17):** token bucket per peer + per-message-type caps + violation window + ban threshold (`crates/strangecoin-net/src/rate_limiter.rs`, ADR-0023, заменил sliding window); production wiring в `peer/connection_loop.rs:216-244` (`check_rate` → `PeerManager::apply_rate_verdict` → `record_rate_violation` / `ban_peer`, persist в `network.json: banned_peers`); интеграция со scoring (ADR-0022) | `rate_limiter.rs` unit; `tests/rate_limit.rs`; `tests/net_rate_limit_integration.rs::spammer_1000_inv_per_second_banned_after_five_violations_and_persists`, `::legitimate_peer_under_100_inv_per_10s_never_banned` | ✅ Stage 2 ext |
| 18 | **Event log:** каждый tx имеет `Receipt`; receipts неизменны | deferred Stage 1.5 | deferred Stage 1.5 | core `vm/result.rs::Receipt` + sub-trie + `BlockHeader.receipts_root` (полная деталь — stage1.5 doc #18) | без изменений (receipts/VM не тронуты Stage 2) | core `tests/receipts.rs` (13); vm-wasm `tests/events_and_receipts.rs` (10); e2e `tests/receipts.rs` (4); `tests/vm_deploy_call.rs`; TLA+ `ReceiptsRootMatch` | ✅ regression |
| 19 | **State root match:** `state.root_after(block) == block.state_root`; иначе reject | deferred Stage 1 | core `state/mod.rs::root_after`; `block_executor.rs::validate_and_apply` | extension: contract storage в глобальном root + `receipts_root` validation (полная деталь — stage1.5 doc #19) | без изменений (state-слой не тронут; gossip/relay-пути применяют блоки через тот же `validate_and_apply`) | core `tests/contract_storage_smt.rs` (9); vm-wasm `tests/contract_storage.rs` (11); core `tests/state_root.rs`; `tests/state_root.rs`; `tests/block_executor.rs` | ✅ regression |
| 20 | **Fee invariant:** `fee_burned + fee_to_miner = total_fees`; `gas_used <= block_gas_limit` | deferred Stage 5 | fee_market stub | degenerate: fee=0 → тривиально; gas-половина через #15 | без изменений (degenerate; Stage 2 не трогает economics) | gas-половина через `tests/vm_gas_exhaustion.rs` + block_executor unit; fee-split trivial при fee=0 | ✅ degenerate (fee market — Stage 5) |
| 21 | **Consensus versioning:** `block.consensus_version <= current_version`; активация по высоте | deferred Stage 1 | core `governance/scip.rs`; `consensus_manager.rs`; `block_executor.rs:109` | без изменений | без изменений (Stage 2 — network-layer; consensus-правила не менялись; pre-tag прямые правки регtest/devnet не потребовались) | `tests/consensus_version.rs` (3); core scip tests (5); `tests/block_executor.rs::rejects_stale_consensus_version` | ✅ regression |
| 22 | **Reproducible builds:** CI публикует SLSA provenance + cosign signature | `.github/workflows/release.yml` | release.yml (LTO, remap-path-prefix, cosign, SLSA L3) | без изменений; vm-wasm входит в тот же release | расширение: CI fuzz matrix + 4 network fuzz targets (S2-P21, `ci.yml`); release-pipeline v2.0.0-stage2 — S2-P24 (до тега regression остаётся на верифицированном v1.5.0-stage1.5 run) | run 38071514479 (v0.0.1-rc1); run 39110855701 (v1.5.0-stage1.5, 13/13); `docs/security/REPRODUCIBLE_BUILDS.md` | ✅ regression (S2-P24 extension pending) |

### Registering notes (S2-P23)

- **#16** — extension: Noise AEAD-фрейминг добавляет второй слой проверки размеров (decrypt → decrypt inner length-prefix → `MAX_MESSAGE_SIZE` до аллокации); replay-защита через monotonic nonces (`NoiseError::Replay`). Место: `crates/strangecoin-net/src/protocol.rs` + `noise/transport.rs`.
- **#17** — extension: token bucket (ADR-0023) заменил Stage 1 sliding window; ban state machine интегрирована со scoring (ADR-0022): rate violation → `record_rate_violation` → score decrement → warn/temp/persistent ban ladder; persistent баны переживают рестарт через `network.json: banned_peers` (`peer/store.rs`).
- **#7/#10/#14/#16/#17** — enforcement-места переехали/расширились в `crates/strangecoin-net/` (S2-P06 миграция + S2-P19 production wiring); слабостей не добавлено.
- **#10** — двойной anti-replay: consensus-level `chain_id` (без изменений) + Noise-transport nonces (новый network-layer слой).
- **#9** — без ослабления: node-Ed25519-ключ segregated в `keystore/node/` (прецедент S2-P09); секреты в config по-прежнему запрещены.
- **#20** — degenerate (fee=0) — без изменений; fee market Stage 5.
- **#22** — Stage 2 release-pipeline extension (v2.0.0-stage2, SHA256SUMS + cosign + SLSA L3) приходит с S2-P24; на момент S2-P23 regression опирается на верифицированные runs Stage 1.5.
- Нумерация остаётся 1:1 с ARCHITECT3 §5 — Stage 2 не добавляет новых инвариантов.

## Regression Audit Results (S2-P23, 2026-10-29)

- **Compilation / Type Safety**: ✅ `cargo clippy --all-targets -- -D warnings` exit 0 (in clippy-p23.log)
- **Workspace suite** (чанками, Windows host preamble + `cmd /c` redirect, см. AGENTS.md gotchas): ✅ все чанки green, 0 failed:
  - `cargo test -p strangecoin` — exit 0 (root: все integration suites ok, включая `net_noise_handshake`/`net_gossip`/`net_compact_blocks`/`net_erlay`/`net_peer_scoring`/`net_rate_limit_integration` + regression `network`/`network_id`/`rate_limit`/`sync_engine`/`sync_headers`/`two_clients`/`rbf`/`double_spend`/`reorg`/`concurrency`)
  - `cargo test -p strangecoin-core` — exit 0, 15 suites ok (core consensus/state/receipts/proptest regression — инварианты #2..#6, #11..#13, #15, #18..#21)
  - `cargo test -p strangecoin-vm-wasm` — exit 0, 10 suites ok (VM regression — инварианты #15/#18/#19/#20 gas-половина)
  - `cargo test -p strangecoin-net` — exit 0, 18 suites ok, **192 tests passed** (network crate: noise/gossip/erlay/scoring/rate_limit/connection_loop/async_migration/shadow + unit)
  - `cargo test --workspace --doc` — exit 0, 4 suites ok
- **Fuzz 9 targets × 600s**: ⚠️ **не верифицировано локально + причина** — хост Windows, cargo-fuzz недоступен (libFuzzer C++ требует clang/MSVC; ASan unsupported на `x86_64-pc-windows-gnu`); сборка/прогон — CI-side (jobs `fuzz-canonical-decode` + Stage 1.5 VM targets + Stage 2 network targets: `noise_handshake`, `gossip_inv`, `compact_blocks_fill_up`, `erlay_reconciliation` — matrix 9 targets, S2-P21 `ci.yml`). Прецедент FBP08/FBP09.
- **No regressions**: ✅ 22/22 enforced; расширения #16/#17 — S2-P08/S2-P16/S2-P17; regression-тесты S2-P22 (`tests/net_*.rs`, 6 файлов) + fuzz-таргеты S2-P21 покрывают network-слой.

## Residual / Deferred

| # | Residual | Fix path |
|---|----------|----------|
| 20 | Fee market stub: fee=0, `fee_burned + fee_to_miner` trivially | Stage 5 (EIP-1559-style dynamic base fee; SCIP до mainnet freeze) |
| 19 | True Verkle/KZG (вместо binary SMT depth 256) | Stage 3+ (ADR-0006 amended) |
| — | Snapshot sync / RocksDB migration / Block-STM / stateless witness wire | Stage 3+ (`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`) |
| — | Receipts prune / contract storage prune | Stage 3+ через SCIP |
| — | Constructor gas/fee burning | Stage 5 fee market |
| — | TLA+ формальная верификация network protocol | Stage 6 (`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`) |
| — | 2 ops-gate (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis key) | mainnet freeze (human ops) |

## Historical Context

- **Stage 0**: `docs/stage0/INVARIANTS_ENFORCED.md` (17/5/0)
- **Stage 1**: `docs/stage1/INVARIANTS_ENFORCED.md` (19/0/2/1)
- **Stage 1.5**: `docs/stage1.5/INVARIANTS_ENFORCED.md` (22/0/0 после S1.5-FBP08)
- **Stage 2**: this document (22/0/0 после S2-P23: #16/#17 extensions, 20 regression, #20 degenerate)
- **Migration**: Strangler — core в `strangecoin-core`, network runtime в `strangecoin-net` (S2-P05/S2-P06), монолит — orchestrator

---

**Related**: `docs/stage1.5/INVARIANTS_ENFORCED.md` · `docs/stage2/THREAT_MODEL_STAGE2.md` · `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` · `docs/security/THREAT_MODEL.md` v3.2 · `analytics/prompt-stage2.md` S2-P23 · ADR-0012..0016, ADR-0020..0023 · `fuzz/README.md`
