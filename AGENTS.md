# AGENTS.md — Strangecoin Developer Guide

> **NOTE — Stage 2 in progress.** Этот файл ведётся инкрементально по правилу FBP10 из retro §8.1 (`prompt-stage2.md` §6.5): Stage 2 секции добавляются только в тех же коммитах, где уже существуют соответствующие артефакты (см. §Forbidden Updates). Секции §Active Stage, §Stage 2 Dependency Map, §Stage 2 Pin Policy, §Stage 2 Determinism Checklist, §Stage 2 Pitfalls **активны** (артефакты S2-P01..S2-P17 существуют: ADR-0020..0023 + `crates/strangecoin-net/` scaffolding + миграция network-модулей + Noise handshake/transport/auth `noise/{mod,handshake,transport,auth,ephemeral}.rs` (`encrypt_frame`/`decrypt_frame`, `wallet::load_or_create_node_keypair`) + HELLO `protocol/hello.rs` + discovery `discovery/{mod,seeds,dnssec,addr_gossip}.rs` + peer management `peer/{mod,store,manager,pool}.rs` + gossip `gossip/{mod,inv,get_data,broadcast}.rs` + compact blocks `gossip/compact_blocks.rs` + Erlay `gossip/erlay/{mod,reconcile,mincut,fallback}.rs` + token-bucket rate limiter `rate_limiter.rs` + peer scoring `peer/scoring.rs`).

## Project Overview

Strangecoin — Rust cryptocurrency (**v1.5.0**, edition 2021): PoW blockchain с secp256k1 подписями, blake3 хешированием, LevelDB хранилищем. **Stage 1.5 завершён** (tag `v1.5.0-stage1.5` на origin → commit `1f133c7`, подтверждён через `git ls-remote --tags origin`): WASM smart contracts через `crates/strangecoin-vm-wasm` (wasmi `=2.0.0`, 13 host functions, 7 precompiles, JSON ABI, gas metering, reentrancy guard, limits, receipts/events, contract storage в SMT). **Stage 2 активен** (`analytics/prompt-stage2.md` v1.0 от 2026-10-21, 24 промпта S2-P01..S2-P24): production-grade P2P в новом крейте `crates/strangecoin-net/` — Noise Protocol Framework, gossip через INV, compact blocks BIP 152, Erlay reconciliation, peer discovery + DNSSEC + ADDR gossip, peer scoring + rate limiting, deprecated v0.8.6 messages cleanup, async migration network workers, shadow network tooling. **Stage 1** завершён ранее (tag `v1.1.0-stage1`, `docs/stage1/STAGE1_SUMMARY.md`): pure core, Sparse Merkle state root, headers-first sync, EventBus, bech32, tokio, 5-component blockchain split. Stage 0 закрыт через debt prompts D01–D03 (`docs/stage0/STAGE0_SUMMARY.md`).

**Residual obligations остаются**: `docs/stage1.5/STAGE1_5_SUMMARY.md` §4 (Stage 2+ deferred list + 2 ops-gates для mainnet freeze: BUG-S1-013 PGP-key offline keygen, BUG-S1-015 offline genesis key air-gapped keygen — НЕ блокируют Stage 2, regtest/devnet scope) и `docs/stage1/STAGE1_SUMMARY.md` §6. Полное закрытие residual Stage 2 — S2-P23 (`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`), Stage 2 DoD верификация — S2-P24 (tag `v2.0.0-stage2` — Gate для Stage 3 RocksDB и Stage 4 dev-experience).

**Key docs**: `analytics/prompt-stage0.md`, `analytics/prompt-stage1.md`, `analytics/prompt-stage1.5.md`, **`analytics/prompt-stage2.md`** (актуальный Stage 2 backlog, 24 промпта), `analytics/ARCHITECT3.md`, `analytics/ROADMAP3.md` (de-facto Stage 2 backlog т.к. `analytics/BACKLOG3.md` не существует), `docs/ADR/`, `docs/stage1.5/STAGE1_5_SUMMARY.md`, `docs/stage1.5/INVARIANTS_ENFORCED.md`, `docs/stage1.5/THREAT_MODEL_STAGE1_5.md`, `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md`, `docs/security/THREAT_MODEL.md`, `docs/spec/consensus.tla` (TLA+ baseline, 35,207 состояний PASS, 10 инвариантов incl. `ReceiptsRootMatch`).

---

## Active Stage: Stage 2 (Сетевая зрелость)

**Status**: in progress (precondition verified — tag `v1.5.0-stage1.5` confirmed on origin commit `1f133c7`; `bugfixes-stage1.5.md` v1.2 verified 11/11 fixed, см. `prompt-stage2.md` §0.1).

**Next prompt**: S2-P18 (deprecated v0.8.6 messages cleanup) ∥ S2-P19 (async migration network workers) — параллельны; опционально S2-P20 (shadow network). Открыты параллельные треки: Cleanup S2-P18∥S2-P19, Shadow S2-P20 (Noise S2-P07..S2-P09, discovery S2-P11, peer S2-P12, gossip S2-P13/S2-P14/S2-P15, rate limiting S2-P16, peer scoring S2-P17 закрыты).

**Prompts done**: 17 / 24 (S2-P01..S2-P17).

**Track status** (см. §Stage 2 Dependency Map ниже для деталей):
- [x] ADRs track: S2-P01 (ADR-0020) / S2-P02 (ADR-0021) / S2-P03 (ADR-0022) / S2-P04 (ADR-0023) — done
- [x] Crate track: S2-P05 (scaffolding `crates/strangecoin-net/` + workspace member + feature `net` default-on) / S2-P06 (migration `src/network/*` → strangecoin-net + re-export shim + `BlockchainView`/`EventSink` cycle break) — done
- [x] Noise track: S2-P07 (handshake `NoiseHandshake`, XX 3 round trips, `expect_remote_static`) / S2-P08 (transport `NoiseTransport`: encrypt/decrypt, monotonic nonces, `NoiseError::Replay`, rekey; framing `encrypt_frame`/`decrypt_frame`, chunking >65519 B) / S2-P09 (Ed25519 `NodeId` auth: `noise/auth.rs`, auth blob `[pubkey||sig]` в rounds 2/3, `into_transport → (NoiseTransport, NodeId)`, `wallet::load_or_create_node_keypair`, детерминированный ephemeral `noise/ephemeral.rs`) — done
- [x] Peer track: S2-P10 (HELLO + capabilities negotiation, `protocol/hello.rs`) / S2-P11 (discovery: `discovery/{mod,seeds,dnssec,addr_gossip}.rs`, `AddrBook`/`AddrSender` + bootstrap) / S2-P12 (peer mgmt: `peer/{mod,store,manager,pool}.rs`, ban state machine, `connect_to_peer` = TCP+Noise XX+HELLO, `impl AddrSender for PeerManager`, network.json `known_peers`/`banned_peers`, config.toml peer tunables) — done
- [x] Gossip track: S2-P13 (INV: `gossip/{mod,inv,get_data,broadcast}.rs`, `GossipState` per-peer dedup, `BlockchainView::mempool_tx`/`block_by_hash`, `PeerManager::send_to_peer`, tests 100-peer ≤1s + real-socket announce) / S2-P14 (compact blocks) / S2-P15 (Erlay: `gossip/erlay/{mod,reconcile,mincut,fallback}.rs`, minisketch stub + fallback, tests 800/1000 → 200 delta) — done
- [x] Limits track: S2-P16 (rate limiting: token bucket + per-message caps + ban threshold, ADR-0023) / S2-P17 (peer scoring: `peer/scoring.rs`, ADR-0022 weighted formula + ban ladder + `peer_scoring_*` config) — done
- [ ] Cleanup track: S2-P18 (deprecated msgs) ∥ S2-P19 (async migration) — параллельны после scaffolding
- [ ] Shadow track: S2-P20 — параллелен после S2-P06, опциональный но рекомендуется
- [ ] Verification track: S2-P21 (fuzz) → S2-P22 (integration tests) → S2-P23 (invariants+threat) → S2-P24 (DoD+tag) — строго после всех proper

**Exit gate**: annotated tag `v2.0.0-stage2` (S2-P24). После — Stage 3 (RocksDB) и Stage 4 (dev-experience: SDK, devnet, indexer, JSON-RPC eth_*, faucet) могут стартовать. Stage 4 требует Stage 2 завершённым (network для dev-experience); Stage 3 — мягкая зависимость (может идти параллельно с Stage 4 согласно ROADMAP3).

---

## Stage 2 Dependency Map

8 жёстких гейтов (`prompt-stage2.md` §1):

1. **Tag gate = Gate Stage 2**: ни один промпт S2-P01+ не начинается до подтверждения тега `v1.5.0-stage1.5` и закрытия всех S1.5-FBP01..FBP11. ✅ Precondition verified — см. §Active Stage.
2. **ADR до кода**: S2-P05 (crate scaffolding) не начинается без ADR-0020..0023 (S2-P01..S2-P04); S2-P07 (Noise handshake) не без ADR-0020; S2-P15 (Erlay) не без ADR-0021; S2-P17 (scoring) не без ADR-0022; S2-P16 (rate limiting) не без ADR-0023.
3. **Crate scaffolding до миграции**: S2-P06 не без S2-P05 (workspace member).
4. **Noise до peer management**: S2-P12 не без S2-P07..S2-P09 (peer connection требует encrypted transport).
5. **Peer management до gossip**: S2-P13 не без S2-P12 (gossip использует peer_manager для broadcast).
6. **Gossip до Erlay**: S2-P15 не без S2-P14 (Erlay reconciliation строится поверх compact-block-aware mempool).
7. **All proper до verification**: S2-P21 не без S2-P01..S2-P20; S2-P22 не без S2-P21; S2-P23 не без S2-P22; S2-P24 не без S2-P23.
8. **S2-P24 = Gate Stage 3/4**: Stage 3 и Stage 4 не начинаются до тега `v2.0.0-stage2`.

Параллельные треки: (a) ADRs S2-P01..S2-P04 — parallel after tag gate; (b) Crate S2-P05 → S2-P06 sequential; (c) Noise S2-P07 → S2-P08 → S2-P09 sequential; (d) Peer S2-P10 → S2-P11 → S2-P12 sequential; (e) Gossip S2-P13 → S2-P14 → S2-P15 sequential; (f) Limits S2-P16 ∥ S2-P17 parallel after peer; (g) Cleanup S2-P18 ∥ S2-P19 parallel after scaffolding; (h) Shadow S2-P20 parallel after scaffolding (optional); (i) Verification S2-P21 → S2-P22 → S2-P23 → S2-P24 strictly after all proper prompts; S2-P24 — gate for Stage 3/4.

---

## Stage 2 Pin Policy

**Правило**: все crate-зависимости, затрагивающие протокол / консенсус / криптографию / network transport, **pinned через `=`** (не `^`, не `*`, не patch-version range). Обновление pinned версии — только через SCIP + activation height **после** mainnet freeze. Аналогично существующему правилу для wasmi `=2.0.0` (ADR-0014/0015).

**Stage 2 pinned dependencies** (предстоящие, добавляются в `crates/strangecoin-net/Cargo.toml` по мере выполнения промптов):

| Crate | Pin | ADR | Промпт | Назначение |
|-------|-----|-----|---------|------------|
| `snow` | `=0.10.0` | ADR-0020 | S2-P07 | Noise Protocol Framework (XX handshake, ChaCha20-Poly1305 transport) |
| `ed25519-dalek` | `=2.0` (резолвится в 2.0.0; lock унифицирован со `strangecoin-vm-wasm`, req `"2"`) | ADR-0020 | S2-P09 | Ed25519 internal P2P identity (node_id auth) |
| `hkdf` | `=0.12.1` | ADR-0020 | S2-P09 | HKDF-SHA256 для детерминированного вывода Noise ephemeral keys (чек-лист #3) |
| `sha2` | `=0.10.9` | ADR-0020 | S2-P09 | Sha256 для `hkdf` (на handshake key-derivation path) |
| `minisketch-rs` | `=0.1.9` (optional, default-off; активная impl — documented stub, см. Статус S2-P15) | ADR-0021 | S2-P15 | Erlay reconciliation (PinSketch sketches, mincut protocol) |
| `siphasher` | `=1.0.4` | — | S2-P14 | Compact blocks BIP 152 (siphash short tx IDs) |
| `hickory-resolver` | `=0.26.3` (features `dnssec-ring`, дефолтные `system-config`/`tokio`) | — | S2-P11 | DNSSEC validation для peer discovery |
| `chacha20poly1305` | через `snow` (не напрямую) | ADR-0020 | S2-P08 | AEAD transport encryption |

**Почему pin**: Noise API может меняться между majors (snow 0.9 → 0.10 breaking); minisketch-rs C++ bindings чувствительны к версии cc-rs; ed25519-dalek 2.0 имеет breaking API vs 1.x; hkdf/sha2 сидят на handshake key-derivation path (детерминизм S2-P09); hickory-resolver ломает API между минорами (0.24 → 0.26 удалил `TokioAsyncResolver`/`ResolveError`, ввёл `TokioRuntimeProvider`/`NetError`) и сидит на trust-пути DNSSEC-валидации seed'ов (V-16). После mainnet freeze любая смена pinned version = SCIP с activation height, чтобы все узлы обновились синхронно.

---

## Stage 2 Determinism Checklist

**Критический инвариант V-21 extension для network layer** (`prompt-stage2.md` §0 строки 129–133, §6.7 строка 1621): каждая новая network-функция обязана быть детерминированной — одинаковый input → одинаковый output. Ephemeral keys генерируются на старте узла из long-term seed (keystore), **НЕ** из system RNG в момент handshake.

5-пунктовый чек-лист применяется к каждой новой network-функции (S2-P07 Noise handshake, S2-P08 transport, S2-P15 Erlay sketches, S2-P17 peer scoring):

1. **Нет `SystemTime::now()` / `Instant::now()` в hot-path**. Таймстампы берутся из `BlockHeader.timestamp` (consensus layer), не из системных часов. Для TTL-логики (ban expiry, peer scoring decay) — только block height.
2. **Нет `thread_rng()` / `OsRng` в handshake/sketch/scoring**. Все keys/sketches/salt — из `keystore.long_term_seed` через HKDF derivation. `OsRng` разрешён только для one-time keypair generation на старте узла (offline, до запуска network).
3. **Ephemeral keys из long-term seed**: Noise ephemeral keypair — HKDF(seed=long_term_node_seed, info=b"noise_ephemeral_v1", counter=node_start_counter). Это обеспечивает forward secrecy (ephemeral keys перегенерируются на каждом старте) + reproducibility (один и тот же seed → та же последовательность ephemeral keys).
4. **Сортировка peer lists по детерминированному ключу** (node_id lexicographic), **не** по времени получения. Если два узла получили peer list в разном порядке — итоговый отсортированный список должен совпадать.
5. **`cargo test --features deterministic_replay` round-trips identically across runs** (используется fixed-seed RNG в tests). Если новая функция ломает deterministic_replay тест — она не детерминирована.

Ревью каждой новой network-функции на детерминизм — **обязательный пункт КГ** в каждом из S2-P07/P08/P09/P15/P17 (см. `prompt-stage2.md` §0 строка 132).

**Статус S2-P07 (2026-10-26) — deferred obligation по пунктам 2–3 закрыт в S2-P09**: сигнатура `NoiseHandshake::initiator/responder(static_key)` была зафиксирована спекой промпта S2-P07, поэтому эфемерный X25519-ключ шёл из snow default resolver (OS CSPRNG) — чек-лист это запрещает. Решение принято явно при выполнении S2-P07 и зафиксировано в commit message (правило «отклонения — явно»). Закрыто вместе с keystore-интеграцией (`NodeKeypair::from_keystore` + `wallet::load_or_create_node_keypair`): HKDF(seed=`keystore.long_term_seed`, `info=b"noise_ephemeral_v1" || counter`) → собственный `snow::resolvers::CryptoResolver` (`noise/ephemeral.rs`) вместо default. Пункты 1/4/5 S2-P07 не затронуты (нет time- и peer-list-логики).

**Статус S2-P08 (2026-10-26)**: obligation пунктов 2–3 не закрыт на момент S2-P08 (тот же дефолтный resolver) — закрыт в S2-P09 (см. статус выше); сами transport-функции чек-лист не нарушают: нет time/RNG/I/O, nonce-счётчики стартуют с 0 и двигаются ровно на 1 за кадр, ключи — из handshake transcript. Rekey-расписание (N сообщений / M байт, синхронизируемое между пирами, noise rev1 §11.3) — примитив `NoiseTransport::rekey()` готов, триггер придёт с connection loop (S2-P12/S2-P19).

**Статус S2-P09 (2026-10-26) — obligation пунктов 2–3 закрыт**: эфемерные ключи Noise больше не берут OS CSPRNG — `SeedResolver` (`noise/ephemeral.rs`) подменяет только RNG snow's resolver (DH/hash/cipher — default), seed = `HKDF-SHA256(long_term_seed, info = b"noise_ephemeral_v1" || counter_be)`, стрим — blake3 в counter mode. Счётчик живёт в `Arc`-внутренности `NodeKeypair` (fetch_add на каждый handshake): одинаковый seed + тот же порядок handshakes → та же последовательность ephemeral keys (reproducibility), каждый следующий handshake → новый ключ (forward secrecy в пределах прогона узла). `OsRng` — только при однократном создании `keystore/node/node_key.json` на старте (разрешено чек-листом). Тест: `tests/noise_auth.rs::ephemeral_keys_are_reproducible_per_seed_and_fresh_per_handshake` + unit-тесты `noise/ephemeral.rs`. Trade-off (зафиксирован явно): эфемер выводится из long-term seed, поэтому компрометация seed раскрывает записанные сессии этого узла — принято ради reproducibility (§0 требует «один и тот же seed → та же последовательность»); верификация — S2-P23/S2-P24. Обязательный пункт КГ-ревью на детерминизм выполнен (нет time/RNG/I/O в handshake-пути, счётчики + transcript-only ключи).

**Статус S2-P11 (2026-10-26)**: чек-лист не обязателен для S2-P11 (в списке §0 только 07/08/09/15/17), ревью выполнено добровольно. Пункт 4 выполняется: `bootstrap` дедуплицирует и сортирует кандидатов по `(addr, node_id)`, `AddrBook::top_k_sorted` / `broadcast_known_addrs` — по `node_id` (NodeId получил `Ord`); повторный прогон с теми же входами → тот же упорядоченный список (`bootstrap_merges_gossip_and_seeds_deterministically`). RNG нет нигде (включая ADDR-рассылку — вместо «случайным пирам» из спеки детерминированный round-robin по отсортированным recipients, отклонение зафиксировано в коммите); `Instant`/`SystemTime` на discovery-путях нет (бюджет 30s — параметр, `tokio::time::timeout` + симулированный clock в тестах). Осознанное ограничение: список жёстких seed-адресов не ротируется автоматически (rotation of seeds из V-16) — ручная ротация через `network.json`, автоматическая отложена до S2-P12/S2-P17 (peer store с TTL).

**Статус S2-P12 (2026-10-26)**: чек-лист не обязателен для S2-P12 (в списке §0 только 07/08/09/15/17), ревью выполнено добровольно. Пункты 1/2 выполняются: scoring — чистая функция счётчиков (`PeerStore::update_score`, скелет), нет `SystemTime`/`Instant`/RNG на scoring-путях; wall clock только на edges (сравнение `ban_until < now` в `tick_at` + age/remaining-stamps при load/save `network.json` — ban policy, не consensus state). Тесты инжекят часы (`tick_at(t0 + 3601s)` вместо сна). Пункт 4: итерации (broadcast, tick, persist, `known_addr_entries`) сортированы по `node_id` — одинаковый набор → одинаковый порядок. Noise static X25519 для dial выводится детерминированно (`HKDF(seed, info=b"noise_static_v1")`, `noise/ephemeral.rs`), без OS CSPRNG; `connect_to_peer` без RNG. Осознанный scope: read-loop входящих сообщений и backpressure (`try_write` WouldBlock) отложены на S2-P19 — детерминизм не затронут.

**Статус S2-P13 (2026-10-26)**: чек-лист не обязателен для S2-P13 (в списке §0 только 07/08/09/15/17), ревью выполнено добровольно. Пункты 1/2/3 выполняются: кодеки (`gossip/{inv,get_data}.rs`, `GossipMessage`) — чистый byte-shuffling, нет `SystemTime`/`Instant`/`RNG`/I/O; `GossipState` — `HashMap<NodeId, HashSet<InventoryItem>>` без clock/RNG (cap 4096/peer, clear-on-overflow — паттерн `SEEN_CAP`); `announce_*`/`handle_*` не таймстампятся. Пункт 4: `announce_*` итерирует `PeerManager::connected_peers()` (sorted by `node_id` из `ConnectionPool::node_ids_sorted`) — детерминированный порядок отправки. Осознанный scope: txid-lookup покрывает только mempool (`chain_contains_tx` требует полной tx — lookup по mined-txid отсутствует, задокументировано в `broadcast.rs`); межпроцессная доставка (read-loop) — S2-P19.

**Статус S2-P14 (2026-10-27)**: чек-лист не обязателен для S2-P14 (в списке §0 только 07/08/09/15/17), ревью выполнено добровольно. Пункты 1/2/3 выполняются: кодеки (`gossip/compact_blocks.rs`) + `short_id` — чистые функции без `SystemTime`/`Instant`/`RNG`/I/O; compact-block nonce **детерминированно** выводится из block hash (`derive_nonce`, отклонение от random-nonce BIP 152 зафиксировано в module docs — диктуется пунктом 2); `fill_up`/`apply_block_txn` не таймстампятся, мемпул-резолв итерирует `mempool_txs()` (порядок не влияет на результат — сверка через merkle `tx_root`). Пункт 5: codec round-trip + fill-up тесты воспроизводимы между прогонами (нет RNG). Осознанный scope: отправка compact-block через live connection loop и per-peer capability gating — S2-P19 (connection loop ещё не читает входящие сообщения).

**Статус S2-P15 (2026-10-27)**: чек-лист **обязателен** для S2-P15 (в списке §0). Пункты 1/2/3 выполняются: `mincut.rs` (`create_sketch`/`decode_difference`/`short_id`) — чистые функции (сортировка + two-pointer XOR + byte encoding), без `SystemTime`/`Instant`/RNG/I/O; элементы валидируются строго-возрастающими при parse (тот же input → тот же output на обеих сторонах); TxId→element = truncation first-8-bytes blake3 (fixed, salt-free encoding, ADR-0021), zero-element исключён (fallback на INV-announce path). `reconcile_at` читает `Instant` только для stamp триггера (edge, паттерн `tick_at`), скетчи и decode остаются pure; resolved TxId-списки сортируются lexicographically перед передачей (ADR-0021 #3), fallback-inventory сортируется по txid (checklist #4). Пункт 5: `deterministic_replay` — feature в workspace отсутствует (doc-only ссылка), воспроизводимость покрыта byte-identical sketch + identical decode тестами (`cargo test -p strangecoin-net` 64 lib + 5 erlay). **Отклонения от literal спеки (FBP, зафиксированы в `erlay/mod.rs` + коммите)**: (a) `minisketch-rs =0.1.9` optional default-off (bindgen 0.49 требует libclang, на хосте нет — verified 2026-10-27), активная impl = documented capacity-bounded stub (ADR-0021 escape hatch, `impl_id=0`; `impl_id=1` зарезервирован за SCIP-gated PinSketch); (b) дефолты триггера 60s/1000 (ADR-0021) вместо 5s/5000 промпта — ADR принят позже и supersede; (c) `decode_difference` возвращает `Vec<u64>` short ids, не `Vec<TxId>` (32-byte TxId неинвертируем из GF(2^64); каждая сторона резолвит только свою половину diff — `reconcile` возвращает `Vec<TxId>` per prompt); (d) `fallback_full_relay`: +`peer`/`+GossipState`, `mempool: &Mempool` → `&dyn BlockchainView` (прецедент broadcast.rs), chunked INV по `MAX_INV_BATCH`. Осознанный scope: wire-carriage sketch envelope и capability-gating раундов — S2-P19 (connection loop); config.toml surface для `ReconciliationConfig` — с живой wiring.

**Статус S2-P16 (2026-10-28)**: чек-лист **не обязателен** для S2-P16 (в списке §0 только 07/08/09/15/17), ревью выполнено добровольно по ADR-0023 §Determinism. Пункты 1/2 выполняются: refill — чистая арифметика `(bucket state, injected now)`, без `SystemTime`/RNG/фоновых таймеров; clock входит только через `_at` API edge (паттерн `tick_at`/`reconcile_at`), тесты гоняют симулированный clock (0 sleep в тестах `rate_limiter.rs` и `tests/rate_limit.rs`). Пункт 3 не применим (нет ephemeral keys). Пункт 4: peer-списки не сортируются в лимитере (state = per-addr HashMap, порядок не влияет на вердикты). **Отклонения от literal спеки (FBP, зафиксированы в module docs + коммите)**: (a) `PeerRateLimiter` +`banned_until`/`ban_duration` — ADR-0023 «ban 5 min» не выражается через `ban_threshold`/`ban_window` из скетча; (b) отдельные legacy sync-бакеты `get_headers`/`get_blocks` (ADR bucket table) — у 10-variant `MessageType` промпта нет sync-вариантов; +`rate_limit_ban_duration_sec` и `get_headers`/`get_blocks` в `rate_limit_per_type` — сверх списка ключей промпта, но требует ADR-0023 «Tuning surface»; (c) clock-injected `_at` варианты — literal сигнатуры промпта (`try_consume(&mut self, count)`) оставлены как wrappers; (d) burst-тест: буквальный КГ «200 INV OK» противоречит INV=100 burst (ADR-0023 worked example) — main-bucket burst тестируется через тип без саб-бакета (`Hello`), INV cap отдельно 100/101; `create_node_for_test` получает тест-конфиг main=100/10 (Stage-1 эквивалент), чтобы существующий `tests/rate_limit.rs` (флуд 200) банится без правок; (e) `handle_connection`: `Exceeded` → drop message + continue, `Ban` → close (ADR fail-closed «refused, dropped» + накопление violations на одной связи; старое поведение закрывало на первом Exceeded); (f) `apply_rate_verdict(Ban)` → `record_rate_violation` **+** `ban_peer(..., persistent=true)` — persistent-флаг в S2-P12 = «записан на диск», `until` остаётся temp 5 min (КГ «ban state persists in network.json» невыполним иначе при ADR-0022 «temp не пишется»); production-wiring `check_rate` для gossip-типов и `apply_rate_verdict` — S2-P19 (inbound loop, node_id известен после Noise auth), как и seam `record_rate_violation` в S2-P12.

**Статус S2-P17 (2026-10-28)**: чек-лист **обязателен** для S2-P17 (в списке §0). Пункты 1/2 выполняются: `compute_score`/`decide` — чистые функции факторов+конфига (нет `SystemTime`/`Instant`/RNG/I/O); счётчики/EWMA обновляются только от protocol-событий (одинаковая последовательность → одинаковый score); wall clock входит только через edge — инжектируемый `now` в `update_score` (штампует `until`) и сравнение `ban_until < now` в `tick_at` (ban policy, не consensus state). Пункт 3 не применим (нет ephemeral keys). Пункт 4: `tick_at` итерирует `peer_ids_sorted` — порядок не влияет на результат (score зависит только от собственного record). Пункт 5: feature `deterministic_replay` отсутствует (doc-only ссылка, как в S2-P15); воспроизводимость — pure-formula unit-тесты + injected-clock (`cargo test -p strangecoin-net` 80 lib + 7 peer_scoring). **Отклонения от literal спеки (FBP, зафиксированы в module docs + коммите)**: (a) `update_score(record, config, now)` — скетч без `now` не может вычислить `until` для BanDecision (паттерн `tick_at`); (b) `ScoringConfig` +`invalid_full_weight`/`violation_full_weight` — ADR-0022 tuning surface, отсутствуют в скете промпта; (c) uptime = responsive fraction `responsive/observed` (ADR-0022 deviation #4) вместо cumulative `uptime_seconds / MAX_UPTIME_SEC` — поле `uptime_seconds` сохранено как числитель (wire-compat), +`observed_seconds` (`serde(default)` для старых файлов), anchor/flag — in-memory; (d) формула — ADR-0022 (counters вычитаются, quality нормируется на grading-weight max, norms без upper clamp) вместо аддитивного literal скетча (аддитивный вариант инвертирует «score < threshold → ban»); (e) `decide` несёт `SCORE_EPSILON = 1e-6` — f32 rounding ставит exact-boundary score (16 violations → ровно 0.2) чуть ниже порога, epsilon держит knife-edge в lower-severity band (false-positive-first); (f) temp-ban threshold остался под историческим S2-P12 ключом `peer_ban_threshold` (back-compat), остальные keys — `peer_scoring_*`; длительности в `_sec` (TOML-эргономика, прецедент rate_limit); (g) `tick_at` оценивает **всех** peers (ADR «every peer», расширило S2-P12 «только connected»), но активные баны не пере-армируются (иначе `until` продлевался бы каждый tick) и just-unbanned получают grace одно окно (иначе cumulative счётчики ре-банили бы мгновенно — expiry был бы no-op); (h) тест «warn recovers»: recovery lever = улучшение latency (fraction-uptime у responsive пира всегда 1.0 — «uptime grows» из промпта покрыт store-level unit-тестом `ewma_converges_and_uptime_fraction_tracks_responsiveness`).

---

## Stage 2 DoD Quick Reference

12 критериев DoD из ROADMAP3 Этап 2 (`prompt-stage2.md` §4, строки 1530–1550). Финальная верификация — S2-P24.

| # | Критерий | Evidence (после закрытия) |
|---|----------|---------------------------|
| 1 | Gossip работает (test: txs распространяются за ≤1 сек на 100 узлах) | `tests/net_gossip.rs` (S2-P22, 1 node → 100 simulated peers ≤1s, simulated clock) |
| 2 | Compact blocks (test: bandwidth reduction ≥50% vs naive) | `tests/net_compact_blocks.rs` (S2-P22, 100-tx block 1 MB → CompactBlock ~10 KB) |
| 3 | Erlay (test: peer reconciliation успешна) | `tests/net_erlay.rs` (S2-P22, 2 peers 1000 txs, 800 shared → 200 transmitted, ≥80% reduction) |
| 4 | Peer discovery: ≥8 peers ≤30 сек | `tests/net_helpers.rs` (S2-P22, SimulatedNetwork, multi-source bootstrap) |
| 5 | Handshake: incompatible peers отвергаются | `tests/net_noise_handshake.rs` (S2-P22, incompatible version/network_id rejected) |
| 6 | Peer scoring: malicious peers banned после N violations | `tests/net_peer_scoring.rs` (S2-P22, invalid blocks → temp ban, protocol violations → persistent ban) |
| 7 | Noise encryption: MITM обнаружен | `tests/net_noise_handshake.rs` (S2-P22, MITM with own Noise keys → NodeId mismatch → drop) |
| 8 | Rate limiting: spammer banned | `tests/net_rate_limit_integration.rs` (S2-P22, 1000 INV/s → banned after 5 violations / 60s) |
| 9 | Legacy messages удалены (no v0.8.6 backward compat) | `crates/strangecoin-net/src/protocol.rs` (GetBlockchain/Blockchain removed, S2-P18), `tests/protocol_deprecated.rs` |
| 10 | `crates/strangecoin-net/` создан, сеть перенесена | `crates/strangecoin-net/Cargo.toml` (workspace member, S2-P05), `src/network/mod.rs` (5-line re-export shim, S2-P06) |
| 11 | ADR-0020..0023 написаны (renumbered from ROADMAP3 plan ADR-0016..0019 — ADR-0016 занят Receipts Root in Block Header) | `docs/ADR/0020-noise-protocol-framework.md` (S2-P01), `0021-erlay-integration.md` (S2-P02), `0022-peer-scoring-algorithm.md` (S2-P03), `0023-rate-limiting-strategy.md` (S2-P04) |
| 12 | `Changelog.md` 2.0.0 + tag `v2.0.0-stage2` | `Changelog.md` (секция 2.0.0), `Cargo.toml` version 2.0.0, annotated tag (S2-P24) |

---

## Stage 2 Threat Model Extensions

Stage 2 закрывает следующие угрозы из `docs/security/THREAT_MODEL.md` (STRIDE v3.1, обновление в S2-P23):

| Threat ID | Описание | Mitigation (Stage 2) | Промпт |
|-----------|----------|----------------------|--------|
| **V-1** | Sybil attacks на P2P | Peer scoring 4-factor weighted + ban state machine (S2-P17), multi-source bootstrap (S2-P11) | S2-P11, S2-P17 |
| **V-2** | Eclipse attacks | Hard-coded seed IPs + DNSSEC validation + ADDR gossip (multi-source) | S2-P11 |
| **V-14** | Replay attacks на network messages | Noise transport encryption (ChaCha20-Poly1305 AEAD) + nonce counter | S2-P08 |
| **V-15** | Invalid blocks DoS | Peer scoring invalid block penalty + temporary ban after N violations | S2-P17 |
| **V-16** | Eclipse via DNS poisoning | Hard-coded seed IPs (not just DNS), DNSSEC validation via hickory-resolver, rotation of seeds | S2-P11 |
| **V-17** | Sybil flooding | Peer scoring + rate limiting (token bucket per peer, ban on exceed) | S2-P16, S2-P17 |
| **V-21 extension** | Non-deterministic network layer (форк консенсуса из-за различий в network RNG/handshake timing) | Determinism checklist (см. §Stage 2 Determinism Checklist выше) — обязателен для каждой новой network-функции | S2-P07, S2-P08, S2-P15, S2-P17 |
| **V-24** | Network MITM | Noise Protocol Framework XX pattern (mutual auth), Ed25519 static key for node_id auth, forward secrecy через ephemeral keys | S2-P07, S2-P08, S2-P09 |

**Residual risks (НЕ закрываются в Stage 2, переносятся в `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` в S2-P23)**: TLA+ формальная верификация network protocol (Stage 6), external audit (Stage 6), ZK-VM integration (Stage 6+ research), Block-STM parallel execution (Stage 3+).

---

## Stage 2 Invariant Extensions

22 инварианта из `docs/stage1.5/INVARIANTS_ENFORCED.md` (Stage 0/1/1.5 — 22/22 enforced, #20 fee-split deferred до Stage 5). **Stage 2 расширяет 3 инварианта** (S2-P23 обновит до 4-й колонки в INVARIANTS_ENFORCED.md):

| # | Инвариант | Stage 1.5 baseline | Stage 2 extension |
|---|-----------|-------------------|-------------------|
| **#16** | P2P framing: length-prefixed, проверка размера ДО аллокации | protocol.rs `MAX_MESSAGE_SIZE` check ДО аллокации | **Расширено**: Noise encrypted framing — `encrypt_frame`/`decrypt_frame` with `MAX_MESSAGE_SIZE` check ДО аллокации (S2-P08), `tests/noise_transport.rs` |
| **#17** | Rate limiting: лимит сообщений/сек от одного пира; нарушение → бан | Stage 1 `rate_limiter.rs` per-message-type caps | **Расширено**: token bucket per peer + per-message caps (S2-P16) + peer scoring integration (ban state machine, S2-P17), `tests/net_rate_limit_integration.rs` |
| **#22** | Reproducible builds | release.yml CI gate + cosign signature | **Расширено**: v2.0.0-stage2 release pipeline (S2-P24), SHA256SUMS + cosign verify + SLSA L3 provenance, `docs/security/REPRODUCIBLE_BUILDS.md` |

**Все остальные 19 инвариантов — regression** (без изменений, проверяются в S2-P23). #20 fee invariant по-прежнему deferred до Stage 5.

**Итог Stage 2**: 22/22 invariants enforced (3 расширены: #16, #17, #22; 1 still deferred до Stage 5: #20).

---

## Build & Test Commands

**Windows host preamble (обязателен в КАЖДОМ shell-вызове перед cargo):**

```powershell
$env:PATH = "C:\msys64\mingw64\bin;$env:PATH"; cargo <...>
```

- PATH живёт в рамках одного shell-процесса — префикс добавляется **в том же вызове**, что и cargo (иначе: `error calling dlltool 'dlltool.exe': program not found` на windows-gnu raw-dylib крейтах — windows-sys 0.61, parking_lot — либо молчаливый `gcc exit 1` с пустым stderr в cc-rs build scripts: blake3, secp256k1-sys, wasmtime, eframe).
- msys64 даёт рабочую пару `dlltool.exe` + `as.exe` (rustup self-contained содержит dlltool БЕЗ as, а dlltool из тулчейна не может запустить as из-за кириллического профиля `C:\Users\ЗС` — CreateProcess падает).
- **Запрещено** ставить shim-директории (напр. `target/dlltool-shim`) в PATH перед msys64 — подменённый `as.exe` без своих DLL ломает cc-rs **молча** (gcc exit 1, пустой stderr, сборка падает без внятной ошибки).
- **Запрещено** выставлять `CC_*` / `AR_*` / `CFLAGS` env-vars в сессии — cc-rs делает `rerun-if-env-changed`, это инвалидирует fingerprint'ы ВСЕХ C-build scripts → многоминутная пересборка secp256k1-sys (precomputed_ecmult), blake3, wasmtime, eframe.

```bash
cargo check                      # fast typecheck
cargo test --workspace           # all tests (workspace)
cargo clippy --all-targets -- -D warnings
cargo build
cargo build --release            # ≈30 мин cold с default features (gui+eframe+LTO) — только если требует КГ
cargo run --example canonical_decode_soak   # fuzz smoke (fallback; cargo-fuzz via CI job fuzz-canonical-decode)
```

**Стандартный цикл промпта** (Development Workflow §5): ровно команды из КГ промпта + `cargo test --workspace`. НЕ добавлять сверх КГ `cargo build --release` / repo-wide fmt / повторные полные прогоны без запроса — измеренные затраты на S2-P05: release cold ≈30 мин, полный `cargo test --workspace` ≈10–12 мин, clippy ≈1 мин, check ≈2–4 мин.

**Тайминг-бюджет (измерено, S2-P05, тёплый кэш unless noted):**

| Команда | Время |
|---------|-------|
| `cargo check --workspace` | 2–4 мин (после fmt — полная перепроверка) |
| `cargo clippy --all-targets -- -D warnings` | ≈1 мин |
| `cargo test --workspace` | ≈10–12 мин network-tests (193с + 292с), НО single-run пересек 25 мин на S2-P11 (пересборка + core lib suite 284с) — гонять чанками: `-p strangecoin` (root) → `-p strangecoin-core` → `-p strangecoin-vm-wasm` → `-p strangecoin-net` → `--doc`, каждый `cmd /c "... > target\x.log"` |
| `cargo build --release` | ≈30 мин cold (первый с gui-default: eframe+wgpu+LTO); далее инкрементально + LTO |
| `cargo fmt --all -- --check` | секунды (см. §Common Gotchas про fmt drift) |

После S2-P21 (fuzzing harness): 4 новых network fuzz targets — `noise_handshake`, `gossip_inv`, `compact_blocks_fill_up`, `erlay_reconciliation` (каждый 600s в CI matrix ubuntu-latest, cargo-fuzz 0.13.2). После S2-P22: 6 новых integration test files — `tests/net_{noise_handshake,gossip,compact_blocks,erlay,peer_scoring,rate_limit_integration}.rs` + `tests/common/net_helpers.rs`.

---

## Running the Node

```bash
# Headless runtime (net + vm-wasm default-on; gui default-on после S2-P05 — бинарь включает GUI-код)
STRANGECOIN_WALLET_PASSWORD=xxx cargo run -- --headless
# GUI client (feature gui входит в default после S2-P05; --no-default-features + --features gui для точечной сборки)
cargo run --features gui
```

**Важно про `net` feature** (после S2-P05): `net` — default-on. Серверная сборка `--no-default-features` **НЕ поддерживается на Stage 2** (net обязателен; без него Create/Call txs проходят, но узел не может подключиться к сети). Stage 4+ введёт feature для «headless + offline-tools» mode (light node без net).

---

## Test Notes

- Integration tests live в top-level `tests/` (not in `src/main.rs`)
- Core unit tests: `crates/strangecoin-core/src/**` and `crates/strangecoin-core/tests/`
- VM tests: `crates/strangecoin-vm-wasm/tests/**` (wasmi e2e; `wasmtime` is dev-only differential oracle, never linked into production binary)
- Network tests (после S2-P22): `crates/strangecoin-net/tests/**` + `tests/net_*.rs` + `tests/common/net_helpers.rs`
- Tests create temporary LevelDB instances in system temp dir — not `target/debug/`
- Network tests use `NETWORK_TEST_LOCK` (write `network.json` next to test exe)
- Real TCP nodes, random ports; no mocks
- `proptest` for property tests (core consensus, chain_selector, state round-trip, merkle, witness, abi types)
- Run tests via `cargo test --workspace` (single-package `-p` runs may rebuild dev-deps differently on this Windows host)

---

## Current Architecture (Stage 1.5 → Stage 2 in progress)

```
crates/strangecoin-core/src/     # pure core, 0 I/O
├── lib.rs            # modules + re-exports (incl. VmExecutor)
├── types.rs          # Block, BlockHeader, Transaction (Transfer/Create/Call), AccountState (EOA/Contract)
├── serialize.rs      # canonical binary (blake3), FORMAT_VERSION, merkle_root, receipts
├── consensus.rs      # chain_id, U256, retarget, MTP, CURRENT_CONSENSUS_VERSION, MAX_CODE_SIZE
├── state/            # inner (apply/unapply + apply_block_with_vm), sparse_merkle.rs, contract_storage.rs, witness.rs
├── economics/        # emission.rs, fee_market.rs (Stage 5 stub)
├── governance/       # scip.rs (SCIP + activation height)
├── abi/              # JSON ABI (S1.5-P12): AbiDocument, head/tail encode/decode, blake3 method_selector
├── chain_selector.rs # fork choice: work → timestamp → hash
├── address.rs        # bech32 (sc1/tsc1/rsc1)
├── vm/               # VmExecutor trait, ExecutionContext/ExecutionResult/Receipt, gas (BLOCK_GAS_LIMIT), limits
└── error.rs          # CoreError

crates/strangecoin-vm-wasm/src/  # wasmi runtime (feature vm-wasm, default-on)
├── lib.rs            # WasmiExecutor (register_code, insert_abi, execute; precompile routing)
├── gas.rs            # instruction counting + host-call weights (ADR-0014)
├── host/             # 13 host functions: crypto/ storage/ calls/ context/ logs/
└── precompiles/      # 0x01..0x07 (bls_aggregate NotImplemented until Stage 7)

# EXISTING crate (S2-P05 создал scaffolding; S2-P06 мигрировал
# protocol/rate_limiter/sync/sync_engine; S2-P07..S2-P20 наполняют stubs):
crates/strangecoin-net/src/      # network runtime (feature net, default-on после S2-P05)
├── lib.rs            # pub mod declarations + NetError/BlockchainView re-export; NetworkHandle/NetworkConfig — later
├── error.rs          # NetError: Handshake/Transport/Peer/Protocol/RateLimit/Discovery (S2-P11) + From<io::Error>/From<snow::Error>/From<NoiseError>
├── blockchain_view.rs # trait BlockchainView (S2-P06: разрыв цикла network↔blockchain) + mempool_tx/block_by_hash (S2-P13) + mempool_txs (S2-P14)
├── protocol.rs       # framing + wire codecs (migrated S2-P06; encrypt_frame/decrypt_frame S2-P08)
├── protocol/hello.rs # HELLO message + capabilities negotiation (S2-P10)
├── noise/mod.rs      # NoiseError (S2-P07, Replay — S2-P08; Auth расширен S2-P09) + pub mod auth/handshake/transport + crate-internal ephemeral
├── noise/auth.rs      # NodeId=blake3(pubkey), NodeKeypair (from_keystore/from_seed, sign), verify_node_id — S2-P09
├── noise/ephemeral.rs # SeedResolver: HKDF-выведенный детерминированный ephemeral RNG вместо OS CSPRNG (S2-P09, чек-лист #2/#3) + noise_static_seed (Noise X25519 static = HKDF(seed, "noise_static_v1"), S2-P12)
├── noise/handshake.rs # NoiseHandshake XX state machine (S2-P07) + auth-blob rounds 2/3 → (NoiseTransport, NodeId) (S2-P09)
├── noise/transport.rs # NoiseTransport: AEAD framing, monotonic nonces, anti-replay, rekey (S2-P08)
├── discovery/mod.rs  # multi-source bootstrap: ≥3 sources параллельно, union ≥8 peers за ≤30s бюджет (S2-P11; DoD #4)
├── discovery/seeds.rs # SeedConfig { hard_coded_ips, dns_seeds, dnssec_required, default_port } + load_seeds из network.json (V-16)
├── discovery/dnssec.rs # DnssecResolver (hickory =0.26.3, opts.validate) + apply_dnssec_policy — reject не-Secure answers (S2-P11)
├── discovery/addr_gossip.rs # ADDR wire [0x05] + AddrBook (unverified→mark_verified) + AddrSender trait + broadcast_known_addrs (S2-P11)
├── peer/mod.rs       # store/manager/pool/scoring split (S2-P12; scoring — S2-P17)
├── peer/store.rs     # PeerStore: PeerRecord (uptime fraction: observed/responsive, anchor — S2-P17) + BanRecord/BanReason + network.json known_peers/banned_peers (ADR-0022)
├── peer/scoring.rs   # ADR-0022 weighted formula: ScoringConfig (peer_scoring_* tunables), compute_score/decide/update_score, latency EWMA + uptime accrual, BanDecision ladder (S2-P17)
├── peer/manager.rs   # PeerManager: connect_to_peer = TCP+Noise XX+HELLO dial, ban state machine (warn→temp 1h→persistent 30d, config.scoring), tick/tick_at (clock injection, все peers + grace unbanned, S2-P17), record_{invalid_block,protocol_violation,rate_violation,latency}, apply_rate_verdict (S2-P16), send_to_peer (S2-P13), impl AddrSender (S2-P12)
├── peer/pool.rs      # Connection (transport: Mutex<NoiseTransport> + TcpStream, send_payload = encrypt_frame+try_write) + ConnectionPool: per-IP-range Sybil cap V-17 (IPv4 /24, IPv6 /64) (S2-P12)
├── gossip/           # mod.rs (GossipMessage dispatch + GossipState), inv.rs (INV/TX wire), get_data.rs (GET_DATA/NOTFOUND), broadcast.rs (announce_tx/block + handle_inv/get_data) — S2-P13; compact_blocks.rs (BIP 152 short IDs + fill-up, S2-P14); erlay/ (S2-P15)
├── rate_limiter.rs   # token bucket per peer + per-message caps + violation window + ban threshold (ADR-0023, S2-P16; заменил sliding window S2-P06)
├── sync.rs           # headers-first sync (migrated S2-P06)
├── sync_engine.rs    # sync engine + trait EventSink (migrated S2-P06)
└── shadow.rs         # stub → replay.rs, compare.rs (S2-P20, optional)

src/                  # node monolith (orchestrator)
├── lib.rs            # Node, mining, P2P orchestration (network-импорты идут через network shim → strangecoin-net)
├── blockchain/       # facade, block_executor (gas-limit enforcement), state_cache, chain_selector, consensus_manager; impl BlockchainView for BlockchainFacade (S2-P06)
├── network/          # mod.rs — re-export shim на strangecoin_net::{protocol, rate_limiter, sync, sync_engine} (S2-P06; других файлов больше нет)
├── events.rs         # EventBus (crossbeam) + impl EventSink для SyncEngine (S2-P06)
├── mempool/          # RBF-capable mempool
├── wallet.rs         # secp256k1 keystore (PBKDF2+AES-GCM) + Ed25519 long-term node keypair (S2-P09)
├── config.rs         # allow_grant_blocks flag, TOML config + peer tunables (peer_ban_threshold/max_connections/max_per_ip_range, S2-P12) + peer_scoring_* (S2-P17) + Config::peer_manager_config()
├── storage/          # LevelDB wrapper
└── error.rs          # StrangecoinError + lock ordering docs
```

---

## Key Constraints (from ARCHITECT3.md / ROADMAP3 / prompt-stage2.md)

- **Strangler**: core grows in `strangecoin-core`; network runtime in `strangecoin-net`; monolith remains working orchestrator
- **0 I/O in core**: no fs/net/tokio/leveldb in `crates/strangecoin-core/src`
- **Consensus changes** via SCIP + activation height (post mainnet freeze)
- **Canonical binary serialization** (blake3) — serde_json only for config/api/tooling (ABI JSON documents are tooling-side; calldata bytes through canonical ABI codec)
- **Grant blocks**: only with `allow_grant_blocks=true` (regtest / legacy migration)
- **wasmi pinned** `=2.0.0`; gas constants hardcoded (ADR-0014/0015) — post-freeze changes via SCIP only
- **Stage 2 network dependencies pinned** (см. §Stage 2 Pin Policy): `snow`, `ed25519-dalek =2.0`, `minisketch-rs`, `siphasher`, `hickory-resolver` — все через `=`, update via SCIP only
- **Stage 2 protocol version**: HELLO `MIN_PROTO_VERSION = 2` (`crates/strangecoin-net/src/protocol/hello.rs`, S2-P10; Stage 1.5 = 1) — peers с version < 2 отвергаются; mismatched `network_id` → drop connection (инвариант #10); capabilities = bitfield, negotiation = `local & remote`

**Stage 2 IN-SCOPE** (это Stage 2 work, НЕ anti-goals — устраняет двусмысленность формулировки «Noise/Erlay Stage 2»):
- Noise Protocol Framework (handshake XX + ChaCha20-Poly1305 transport + Ed25519 auth) — S2-P07..S2-P09
- Gossip protocol (INV/GET_DATA/TX/BLOCK) — S2-P13
- Compact blocks BIP 152 — S2-P14
- Erlay reconciliation — S2-P15
- Peer discovery (seed IPs + DNSSEC + ADDR gossip) — S2-P11
- Peer scoring + rate limiting — S2-P16/S2-P17
- Deprecated v0.8.6 messages cleanup — S2-P18
- Async migration network workers — S2-P19
- Shadow network tooling (Stage 6 prep) — S2-P20

**Anti-goals Stage 2+** (явно отложено — `prompt-stage2.md` §7):
- RocksDB migration (LevelDB → RocksDB) — Stage 3
- Stateless validation witnesses (Verkle proofs) — Stage 3
- State pruning / expiry (EIP-4444 analog) — Stage 3
- Snapshot sync — Stage 3
- Block-STM (parallel execution) — Stage 3+
- Account Abstraction (ERC-4337) — Stage 5
- EIP-1559 dynamic base fee — Stage 5
- Threshold encryption mempool impl — Stage 5
- WIT/Wasm Component Model для ABI — Stage 4+
- K-framework формальная верификация WASM subset — Stage 6
- ZK-VM integration (RISC Zero / SP1) — Stage 6+ research
- BLS12-381 dependency + bls_aggregate precompile impl — Stage 7
- PoS migration (Casper FFG) — Stage 7
- SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet — Stage 4
- TLA+ формальная верификация network protocol — Stage 6 (residual risk зафиксирован в S2-P23)
- Multidimensional fees (EIP-7706 analog) — Stage 6+ research
- PBS (Proposer-Builder Separation) — Stage 6+ research
- Batch auctions — Stage 5+ опционально
- Inclusion lists (PoS-фаза) — Stage 7+
- Validator set rotation, slashing, staking rewards — Stage 7
- Metrics (Prometheus/OpenTelemetry), Docker images, systemd unit files — Stage 4
- HD wallets (BIP-39/44), hardware wallet support (Ledger) — Stage 4+
- Constructor fee burning, receipts prune, contract storage prune — Stage 3+ через SCIP

**22 invariants**: `docs/stage1.5/INVARIANTS_ENFORCED.md` (21 enforced + #20 fee-split deferred Stage 5). Stage 2 расширяет #16/#17/#22 (см. §Stage 2 Invariant Extensions).

---

## Lock Ordering (critical — see `src/error.rs`)

1. **blockchain** (via `BlockchainFacade`, internally `RwLock`) — outer, first
2. **peer_manager** (`RwLock` в `crates/strangecoin-net/src/peer/manager.rs` после S2-P12) — middle, second
3. **connection_pool** (`Mutex` в `crates/strangecoin-net/src/peer/pool.rs` после S2-P12) — inner-network, third
4. **wallet** (file-based keystore lock) — inner, last

**Никогда** не брать wallet lock holding blockchain write lock из другого call site. **Никогда** не брать `connection_pool.Mutex` holding `peer_manager.RwLock` write из network worker thread, который одновременно может дернуть `BlockchainFacade` для валидации блока — это создаёт deadlock. Если network worker должен валидировать блок — отпустить `peer_manager` lock до вызова `BlockchainFacade`.

---

## Stage 2 Pitfalls

Ловушки Stage 2, не очевидные из Stage 1.5:

| Pitfall | Решение |
|---------|---------|
| `snow` `HandshakeState` → `TransportState` переход не thread-safe — нужен в одном thread | Wrapper с явной ownership передачей; `Arc<Mutex<>>` только для `TransportState` после handshake завершения |
| `minisketch-rs` собирает C++ через cc-rs — Windows требует MSBuild + env vars | Documented в CI (`.github/workflows/ci.yml` matrix ubuntu-latest); локально — fallback на stub impl (S2-P15 explicitly allows stub с SCIP-gated full impl) |
| Ed25519 vs secp256k1: P2P использует Ed25519 (schnorr-like), wallet — secp256k1 (ECDSA). **Не путать ключи!** | Keystore segregates: `keystore/wallet/` (secp256k1) vs `keystore/node/` (Ed25519 long-term keypair, S2-P09) |
| `hickory-resolver` async-only — ломает sync-тесты Stage 1.5 | Wrapper `DnsResolver` с `Runtime::block_on` для sync API в tests; production — async через tokio runtime |
| `--no-default-features` после Stage 2 **не собирает net** — light node build сломается | Documented в S2-P05; Stage 4+ вводит `headless` feature для offline-tools mode (без net, с CLI tools) |
| `snow` version drift между macOS/Linux/Windows — разные binaries на разных платформах | CI matrix: ubuntu-latest только для fuzz; release pipeline собирает все 3 платформы через cross-rs (S2-P24) |
| `ed25519-dalek =2.0` API breaking vs 1.x — миграция требует rewriting всех signature call sites | S2-P09 единственный consumer; uses 2.0 API с первого дня; wallet (secp256k1) не затрагивается |
| Erlay minisketch field size GF(2^64) vs GF(2^32) — выбор влияет на bandwidth vs reconciliation cost | ADR-0021 (S2-P02) фиксирует GF(2^64) для capacity ≥10k txs; S2-P15 impl с явно documented trade-off |

---

## Forbidden Updates

**Мета-правило FBP10** из retro §8.1 (`prompt-stage2.md` §6.5):

- **НЕ добавлять** Stage 2 секции / промпты / трекеры в AGENTS.md, Changelog.md, README.md **до** соответствующего кодового коммита с артефактом.
- **«Stage 2 complete» объявление** — **только** через annotated tag `v2.0.0-stage2` (S2-P24), никогда через «ощущение завершённости».
- **Non-locally-verifiable факты** (CI runs, fuzz stats, cosign/slsa blob verify, release pipeline) — помечать как «не верифицировано + причина» (прецедент FBP08/FBP11), не молча опускать.
- **Все отклонения от первичной спеки** — фиксировать явно в разделе «Решение» каждого промпта (прецедент FBP03/FBP04/FBP05/FBP07).
- **AGENTS.md обновляется** инкрементально — см. «Последовательность внедрения» ниже.

**Последовательность внедрения** AGENTS.md updates (соблюдение FBP10):
1. После S2-P01 (ADR-0020 committed): добавить §Active Stage, §Stage 2 Dependency Map, §Stage 2 Pin Policy, §Stage 2 Determinism Checklist, ссылку на `analytics/prompt-stage2.md`. Артефакт `docs/ADR/0020-noise-protocol-framework.md` существует → легитимно.
2. После S2-P05 (crate scaffolding): обновить §Current Architecture — добавить `crates/strangecoin-net/` макет; обновить §Common Gotchas — добавить §Stage 2 Pitfalls.
3. После S2-P10 (HELLO): обновить §Key Constraints — `MIN_PROTO_VERSION=2` для Stage 2.
4. После S2-P24 (final tag): заменить «Stage 2 in progress» на «Stage 2 complete (tag `v2.0.0-stage2` — pending S2-P24)»; добавить в §Important Files: `docs/stage2/{STAGE2_SUMMARY,INVARIANTS_ENFORCED,THREAT_MODEL_STAGE2,RESIDUAL_RISKS_STAGE3_PLUS}.md`.

---

## Development Workflow

1. Stage 0 prompts: `analytics/prompt-stage0.md` (done, D01–D03)
2. Stage 1 prompts: `analytics/prompt-stage1.md` (done, S1-P01–S1-P22)
3. Stage 1.5 prompts: `analytics/prompt-stage1.5.md` (done, S1.5-P01–S1.5-P19; P12 gap closed 2026-10-18)
4. **Stage 2 prompts: `analytics/prompt-stage2.md`** (active, S2-P01–S2-P24, 24 промпта, precondition verified — tag `v1.5.0-stage1.5` confirmed + bugfixes-stage1.5.md 11/11 fixed)
5. Each prompt: Windows host preamble (PATH, см. §Build & Test Commands) → implement → ровно команды из КГ промпта + `cargo test --workspace` → verify КГ (критерии готовности) → commit. НЕ расширять верификацию без запроса (release ≈30 мин, повторные полные прогоны ≈10–12 мин каждый — измерено на S2-P05)
6. **Do not commit unless explicitly asked**
7. **ADR before code** that changes architecture; Changelog only after artifacts exist (FBP10 rule, see §Forbidden Updates)
8. **§0 «Общий контекст» блок** из `prompt-stage2.md` — НЕ дублировать в каждом промпте; AGENTS.md уже содержит эквивалентную информацию (§Project Overview + §Active Stage + §Stage 2 Dependency Map + §Key Constraints). Промпты Stage 2 содержат только prompt-specific блок (Цель / Контекст / Задачи / Артефакты / КГ / Коммит) — экономия ~3K токенов на промпт × 24 = ~70K токенов.
9. Stage 3 (RocksDB) / Stage 4 (dev-experience: SDK, devnet, indexer, JSON-RPC eth_*, faucet) — **не начинаются** до тега `v2.0.0-stage2` (S2-P24).

---

## Config & Secrets

- `config.toml` — non-secret config only
- **Never** put passwords/keys in config files
- Wallet password: env var `STRANGECOIN_WALLET_PASSWORD` or interactive prompt
- Keystore: `keystore/*.json` (encrypted PBKDF2+AES-GCM)
- **Ed25519 long-term node keypair** (Stage 2 S2-P09): `keystore/node/` subdirectory — separate from wallet keystore (`keystore/wallet/`). NEVER в config.toml.
- Genesis key residual: seed string is public until offline key replacement (see STAGE1_SUMMARY obligations, BUG-S1-015 ops gate)

---

## Database

- LevelDB at `./data/leveldb/` (configurable via `config.toml` `[storage]` path)
- State cache rebuilt from chain on adoption (invariant #1)
- **RocksDB migration — Stage 3** (anti-goal для Stage 2, см. §Key Constraints)

---

## Important Files to Know

| File | Purpose |
|------|---------|
| **`analytics/prompt-stage2.md`** | **Stage 2 task breakdown (S2-P01..S2-P24)** — актуальный backlog, 24 промпта с КГ |
| `analytics/ROADMAP3.md` | Этапный план — de-facto Stage 2 backlog (BACKLOG3.md не существует); см. «Этап 2 — Сетевая зрелость» |
| `analytics/ARCHITECT3.md` | Целевая архитектура; см. §3.6 network/p2p, §4.2 sync, §5 инварианты, §6 угрозы, §10.4 strangler Stage 2, §13 headless, §14 network_id, §15 anti-goals, §17 PR-checklist |
| `analytics/prompt-stage1.5.md` | Stage 1.5 task breakdown (template/формат для Stage 2) |
| `analytics/bugfixes-stage1.5.md` | Stage 1.5 bugfix prompts (11/11 fixed, precondition для Stage 2) |
| `docs/stage1.5/STAGE1_5_SUMMARY.md` | Stage 1.5 DoD evidence + residual obligations §4 |
| `docs/stage1.5/INVARIANTS_ENFORCED.md` | 22 invariants, Stage 1.5 locations (source of truth); Stage 2 расширит #16/#17/#22 (S2-P23) |
| `docs/stage1.5/THREAT_MODEL_STAGE1_5.md` | V-18/V-19/V-21 mitigations; Stage 2 extends с V-1/V-2/V-14/V-15/V-17/V-24 (S2-P23) |
| `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` | Stage 2+ residual risks (constructor gas burning Stage 5, receipts/contract prune Stage 3+ SCIP) |
| `docs/security/THREAT_MODEL.md` | STRIDE model v3.1 (Stage 0/1/1.5 vectors); Stage 2 обновит V-1/V-2/V-14/V-15/V-17/V-24 mitigations (S2-P23) |
| `docs/spec/consensus.tla` | TLA+ baseline (35,207 states PASS, 10 invariants incl. ReceiptsRootMatch); Stage 2 НЕ расширяет TLA+ для network (residual risk) |
| `docs/ADR/0012..0015` | Stage 1.5 architecture decisions (wasmi, JSON ABI, gas, precompiles) |
| `docs/ADR/0016-receipts-root-in-block-header.md` | **Занятый ADR-0016 слот** (Stage 1.5 FBP07) — Stage 2 ADRs переименованы на 0020..0023 (см. §Stage 2 DoD Quick Reference критерий 11) |
| `docs/ADR/0020-noise-protocol-framework.md` | **S2-P01 done** — Noise Framework (XX handshake, snow crate) |
| `docs/ADR/0021-erlay-integration.md` | **S2-P02 done** — Erlay (mincut reconciliation + fallback) |
| `docs/ADR/0022-peer-scoring-algorithm.md` | **S2-P03 done** — peer scoring (weighted sum, ban state machine) |
| `docs/ADR/0023-rate-limiting-strategy.md` | **S2-P04 done** — rate limiting (token bucket per peer, ban on exceed) |
| **FORTHCOMING**: `docs/stage2/STAGE2_SUMMARY.md` | 12 DoD критериев с evidence — S2-P24 |
| **FORTHCOMING**: `docs/stage2/{INVARIANTS_ENFORCED,THREAT_MODEL_STAGE2,RESIDUAL_RISKS_STAGE3_PLUS}.md` | S2-P23 |
| `crates/strangecoin-vm-wasm/src/lib.rs` | WasmiExecutor (runtime entry point) |
| `crates/strangecoin-core/src/abi/` | JSON ABI codec (encode/decode/selector) |
| `crates/strangecoin-net/src/lib.rs` | Network crate entry point (S2-P05 scaffolding; S2-P06 мигрировал protocol/rate_limiter/sync/sync_engine) |
| `genesis.json` | Genesis block definition |

---

## Common Gotchas

- **Windows paths**: Use `C:\projects\strangecoin` not `/c/projects/strangecoin`
- **PowerShell**: Use `;` not `&&` for command chaining
- **dlltool / windows-gnu raw-dylib**: host toolchain `x86_64-pc-windows-gnu` не содержит `as.exe` в self-contained, а `dlltool` из rustup-тулчейна не может запустить `as` (CreateProcess падает из-за кириллического профиля `C:\Users\ЗС`). Рабочее решение — **Windows host preamble** в §Build & Test Commands (`C:\msys64\mingw64\bin` первым в PATH того же вызова, что и cargo). НЕ добавлять `target/dlltool-shim` (закрытый as.exe ломает cc-rs: gcc exit 1 без stderr); НЕ выставлять `CC_*`/`AR_*` env-vars (инвалидация всех C-build fingerprints). CI (ubuntu) не затронут
- **fmt drift (закрыт [S2-P05] style-коммитом)**: до S2-P05 repo-wide `cargo fmt --all -- --check` давал ~300 pre-existing diff'ов (~80 файлов core+src+tests) — дрейф накопился ранее и не был от правок промпта. Закрыто: `cargo fmt --all` принят rustfmt 1.9.0 (gnu), 1.10.0 (= CI stable канал, msvc-тулчейн) и nightly — 0 diff каждым. Если дрейф появится снова (смена rustfmt-канала) — повторить `cargo fmt --all` отдельным style-коммитом; сверять только тронутые файлы (`cargo fmt -p <crate> -- --check`), полный fmt-прогон секунды. CI beta-канал и статус CI-джоб локально **неверифицируемы** (нет `gh` CLI на хосте)
- **Пайплайн-обрыв в PowerShell**: `cargo test ... | Select-String ... | Select-Object -First N` убивает cargo после N совпадений (exit -1, тесты не дорабатывают). Полный прогон: перенаправлять в файл (`... | Out-File target\test.log`) и фильтровать файл после завершения
- **Долгие команды убивает харнесс (наблюдение, S2-P08)**: любая команда с выводом в shell-pipe живёт ~5 мин и гибнет с `ChildProcess.kill` (даже при большем timeout-параметре; `| Out-File` — тоже pipe на стороне PS). Нативная запись в файл через `cmd /c "cargo ... > target\x.log 2>&1"` (дескриптор stdout = файл, не pipe) переживает и 5+ мин — verified на `cargo test -p strangecoin-core --lib` (303 s, exit 0). `cargo test --workspace` целиком (~10–12 мин) через pipe не проходит — гонять чанками (`-p <crate>`, отдельные `--test <name>`) с `cmd /c "... > log"`, результаты собирать grep'ом по логам после каждой команды
- **GUI**: feature `gui` is defined; default-on после S2-P05 (по спеке промпта; headless-режим — `-- --headless` runtime-флаг)
- **cargo-fuzz**: not runnable on this Windows host — use `cargo run --example canonical_decode_soak`; coverage-guided runs live in CI jobs `fuzz-canonical-decode` + Stage 1.5 VM targets + Stage 2 network targets (push/PR)
- **tracing only** (no println!) — структура: `tracing::info!(target = "net::noise", peer_id = %id, "handshake completed")`
- **Network test isolation**: `NETWORK_TEST_LOCK` + `network.json` next to test exe
- **vm-wasm feature**: default-on; light nodes build with `--no-default-features` (no VM, Create/Call txs rejected as `UnsupportedTxType`)
- **net feature** (после S2-P05): default-on; `--no-default-features` **НЕ поддерживается на Stage 2** (без net узел не может P2P). Stage 4+ введёт `headless` feature для offline-tools mode
- **ABI golden vectors**: `tests/abi_roundtrip.rs` pins selector bytes and wire offsets — changing them is a wire-format break
- **snow `HandshakeState` → `TransportState`**: переход не thread-safe — см. §Stage 2 Pitfalls
- **minisketch-rs C++ bindings**: Windows требует MSBuild; локально — stub impl (см. §Stage 2 Pitfalls)
- **Ed25519 vs secp256k1**: P2P использует Ed25519, wallet — secp256k1. Keystore segregates `keystore/node/` vs `keystore/wallet/` (см. §Stage 2 Pitfalls)
- **Network errors — `NetError`, не `StrangecoinError` (S2-P06)**: мигрированные `protocol`/`rate_limiter`/`sync` возвращают `NetError`; маппинг: `SizeLimitExceeded`→`Protocol`, `IoError`→`Transport`, `InvalidHelloMessage`→`Protocol`, `PeerBanned`→`RateLimit`, `InvalidBlock`→`Protocol`. Побочный эффект: префикс категории в лог-строке («size limit exceeded: X» → «protocol error: size limit exceeded: X») — ни один тест на это не ассертит
- **Shim `src/network/mod.rs`**: формула из спеки промпта `pub use strangecoin_net::{mod::*, …}` невалидна (`mod` — keyword); рабочая форма `pub use strangecoin_net::{protocol, rate_limiter, sync, sync_engine}` + item-re-export `RateLimiter`/`MessageClass`. `src/network/` больше не содержит файлов кроме `mod.rs`
- **`Arc::clone(&x)` не коэрсится в `Arc<dyn Trait>`** (вложенная coercion через `&Arc<T>` не проходит): в позиции аргумента передавайте `x.clone()` — unsized coercion применяется к owned value (см. `sync_engine::spawn` в `src/lib.rs`)

---

## PR / Commit Conventions

- **No commits without explicit user request**
- **Commit format**: `[S2-PNN] <area>: <short description>` — где `NN` = 01..24 (zero-padded), `area` = `noise` / `peer` / `gossip` / `rate-limit` / `scoring` / `cleanup` / `async` / `shadow` / `fuzz` / `tests` / `invariants` / `release`. Примеры: `[S2-P07] noise: snow handshake XX state machine`, `[S2-P22] tests: net integration end-to-end`, `[S2-P24] release: stage2 dod verification + tag v2.0.0-stage2`
- **Commits/push** as **Reider85 <reider85@users.noreply.github.com>** (remote: `github.com/Reider85/strangecoin`); verify `git config user.name`/`user.email` before committing
- **ADRs written BEFORE code changes** (S2-P01..S2-P04 — все 4 ADR до S2-P05)
- **Each prompt = one logical change set**; prompt ID in commit message (format выше)
- **AGENTS.md / Changelog.md / README.md updates** — **только в том же коммите**, где уже существует соответствующий артефакт (правило FBP10 из retro §8.1, см. §Forbidden Updates)
- **Annotated tag** `v2.0.0-stage2` — строго после прохождения всех КГ S2-P24; формат сообщения см. в `prompt-stage2.md` S2-P24 строки 1424–1449
