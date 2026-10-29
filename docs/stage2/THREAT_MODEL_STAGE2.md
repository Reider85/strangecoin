# THREAT_MODEL_STAGE2.md — Stage 2 Network-Layer Threats (Summary)

**Document**: `docs/stage2/THREAT_MODEL_STAGE2.md`
**Status**: Complete ✅
**Date**: 2026-10-29 (S2-P23)
**Prompt**: S2-P23 (`analytics/prompt-stage2.md`)
**Full model**: `docs/security/THREAT_MODEL.md` v3.2 (§3.1/§3.5/§3.6/§3.8 — V-01/V-02/V-14/V-15/V-16/V-17/V-24; §6.2c/§6.3/§6.4 — coverage; §7.1c — residual)

Краткое резюме threat-векторов, актуализированных появлением production-grade
P2P (Stage 2: Noise transport, gossip/compact/Erlay, discovery, scoring, rate
limiting — S2-P07..S2-P20 в `crates/strangecoin-net/`). Нумерация векторов —
общий threat model (ARCHITECT3 §6); инварианты — `docs/stage2/INVARIANTS_ENFORCED.md`.

## 1. Закрытые/усиленные векторы (Stage 2 Amendment)

| # | Вектор | STRIDE | Митигация (Stage 2) | Ключевой код | Ключевой тест |
|---|--------|--------|---------------------|--------------|---------------|
| V-24 | Network MITM | Spoofing / Info Disclosure | Noise XX handshake (S2-P07) + ChaCha20-Poly1305 AEAD framing + monotonic nonces anti-replay (S2-P08) + Ed25519 node_id auth (S2-P09, ADR-0020); единый Noise-транспорт, plaintext-путь удалён (S2-P19) | `noise/{handshake,transport,auth,ephemeral}.rs`, `protocol.rs::encrypt_frame/decrypt_frame` | `tests/net_noise_handshake.rs::mitm_with_own_keys_is_dropped_on_node_id_mismatch` |
| V-01 | Eclipse | Spoofing / Info Disclosure | Multi-source bootstrap: hard-coded IPs + DNSSEC + ADDR gossip, ≥8 peers ≤30s (S2-P11); peer scoring + ban ladder (S2-P17); per-IP-range conn cap (S2-P12) | `discovery/{mod,seeds,dnssec,addr_gossip}.rs`, `peer/{pool,scoring,manager}.rs` | `tests/net_peer_scoring.rs`, `tests/net_helpers.rs` |
| V-02 | Partition | DoS | sync_engine ≥3 peers (Stage 1 regression, S2-P06 без behavioral changes); Noise убирает plaintext-MITM partition (V-24) | `sync_engine.rs`, `sync.rs` (Noise short-lived sessions, S2-P19) | `tests/sync_engine.rs`, `tests/sync_headers.rs` (regression) |
| V-14 | DoS: Spam txs | DoS | Token bucket per peer + per-message caps + violation ban (S2-P16, ADR-0023); gossip dedup (S2-P13); интеграция со scoring (S2-P17) | `rate_limiter.rs`, `peer/connection_loop.rs`, `gossip/{broadcast,mod}.rs` | `tests/net_rate_limit_integration.rs::spammer_1000_inv_per_second_banned_after_five_violations_and_persists` |
| V-15 | DoS: Invalid blocks | DoS | Peer scoring: `invalid_blocks_txs_sent` (weight 0.5) → warn → temp ban 1h → persistent ban 30d (S2-P17, ADR-0022); persistent в network.json | `peer/scoring.rs`, `peer/manager.rs::record_invalid_block`, `peer/store.rs` | `tests/net_peer_scoring.rs::malicious_peer_banned_after_configured_invalid_block_threshold` |
| V-16 | Eclipse via DNS poisoning | Spoofing | DNSSEC validation (`DnssecResolver`, hickory-resolver =0.26.3, reject не-Secure) + hard-coded IPs + ADDR gossip (S2-P11) | `discovery/{seeds,dnssec}.rs` | discovery unit (dnssec policy), `tests/net_helpers.rs` (multi-source) |
| V-17 | Sybil | Spoofing | Per-IP-range connection cap IPv4 /24, IPv6 /64, `DEFAULT_MAX_PER_IP_RANGE=8` (S2-P12); scoring + rate-limit bans (S2-P16/S2-P17) | `peer/pool.rs::count_by_ip_range`, `peer/scoring.rs` | `tests/net_peer_scoring.rs`, `tests/net_rate_limit_integration.rs` |

Связанные инварианты (Stage 2 extensions — `docs/stage2/INVARIANTS_ENFORCED.md`):
- **#16** (P2P framing) — extension: Noise encrypted framing, `MAX_MESSAGE_SIZE` до аллокации, anti-replay nonces (S2-P08)
- **#17** (Rate limiting) — extension: token bucket + per-message caps + ban threshold + peer scoring integration (S2-P16/S2-P17)

## 2. Новый attack surface (Stage 2)

| Surface | Описание | Митигация / Status |
|---------|----------|---------------------|
| Noise handshake state machine | 3 round trips; snow `HandshakeState` → `TransportState` переход; state-machine bugs → auth bypass | snow `=0.10.0` (pinned, battle-tested); handshake fuzz target `noise_handshake.rs` (S2-P21, CI-side); unit-тесты `noise/handshake.rs` |
| Ephemeral key derivation из long-term seed | HKDF(seed, info=`noise_ephemeral_v1‖counter`) вместо OS CSPRNG (детерминизм §checklist); компрометация seed раскрывает записанные сессии узла | Осознанный trade-off (S2-P09, reproducibility > per-session FS от seed compromise); forward secrecy в пределах прогона узла (counter freshness); residual зафиксирован §7.1c |
| TOFU unpinned first contact | Unpinned handshake принимается как unverified; MITM на первой встрече возможен | Pinning по node_id после первого успеха; `tests/net_noise_handshake.rs::unpinned_first_contact_is_accepted_as_unverified` документирует поведение |
| DNSSEC resolver (hickory) | Async-only C-крипто (ring); resolver bugs → bad peer sets или DoS при validation | Pin `=0.26.3`; multi-source union — отказ одного источника не изолирует; reject не-Secure answers fail-closed |
| ADDR gossip | Отравленный ADDR может добавить враждебные адреса в AddrBook | AddrBook unverified → mark_verified после успешного handshake; top-K cap; bootstrap-merge дедуплицирует |
| Erlay sketch exchange | Враждебный peer может слать декодируемые-но-ложные sketches → принудительный fallback | Capacity-bounded stub (`impl_id=0`) не декодирует чужие sketches в txids; fallback = полный INV-relay (no tx loss); reconcile triggers rate-bounded (60s/1000, ADR-0021) |
| Compact-block fill-up probing | GET_BLOCK_TXN probes могут вынуждать пересылку mempool-tx | Per-message caps (S2-P16); fill-up только на валидные short-id запросы к известному блоку |
| Plaintext-клиенты Stage 1.5 | Старые узлы несовместимы с единым Noise-транспортом | Допустимо до тега `v2.0.0-stage2` (mainnet не запущен; прецедент S2-P18 cleanup) |

## 3. Determinism (V-21 extension для network layer)

Чек-лист детерминизма (`analytics/prompt-stage2.md` §0) применён к S2-P07/P08/P15/P17;
ревью S2-P09/P11..P14/P16/P18..P20 — в AGENTS.md §Stage 2 Determinism Checklist.

Ключевые свойства:
- Нет `SystemTime`/`Instant`/`thread_rng` в handshake/sketch/scoring hot-path (clock — только injected edges `tick_at`/`reconcile_at`)
- Эфемерные/static Noise-ключи — HKDF от `keystore.long_term_seed` (`noise/ephemeral.rs`), не OS CSPRNG
- Peer-итерации сортированы по `node_id` (детерминированный порядок)
- Compact-block nonce детерминированно выводится из block hash (`derive_nonce`)
- Erlay элементы — truncation first-8-bytes blake3, strict-increasing validation; resolved списки сортируются lexicographically

## 4. Residual Risks Stage 3+

Полный перечень — `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` (S2-P23).
Ключевые network-строки: TLA+ формальная верификация network protocol (Stage 6),
Erlay PinSketch full impl (SCIP-gated), seed rotation automation, TOFU pinning
hardening, external audit (Stage 6).

## 5. Fuzzing + Tests (S2-P21 / S2-P22)

| Target / Suite | Покрытие |
|----------------|----------|
| `fuzz/fuzz_targets/noise_handshake.rs` | Handshake state machine на произвольных байтах (CI-side, 600s) |
| `fuzz/fuzz_targets/gossip_inv.rs` | INV-кодеки + GossipState dedup |
| `fuzz/fuzz_targets/compact_blocks_fill_up.rs` | Short-id + fill-up / apply_block_txn |
| `fuzz/fuzz_targets/erlay_reconciliation.rs` | Sketch create/decode + reconcile determinism |
| `tests/net_noise_handshake.rs` (4) | Roundtrip, MITM NodeId mismatch, TOFU, transport pair |
| `tests/net_gossip.rs` (2) | 100-peer ≤1s propagation, INV dedup |
| `tests/net_compact_blocks.rs` (3) | ≥50% bandwidth reduction, mempool fill-up, GET_BLOCK_TXN |
| `tests/net_erlay.rs` (2) | 800/1000 → 200 delta, fallback без tx loss |
| `tests/net_peer_scoring.rs` (3) | Invalid-block ban, latency false-positive, persistent ban restart |
| `tests/net_rate_limit_integration.rs` (2) | Spammer ban, legitimate peer never banned |

cargo-fuzz локально недоступен (Windows host) — CI matrix 9 targets ×600s;
локальная верификация — type-check + zeroed-input run harness (см. `fuzz/README.md`).

---

**Related**: `docs/security/THREAT_MODEL.md` v3.2 · `docs/stage2/INVARIANTS_ENFORCED.md` · `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` · `docs/ADR/0020-noise-protocol-framework.md` · `docs/ADR/0021-erlay-integration.md` · `docs/ADR/0022-peer-scoring-algorithm.md` · `docs/ADR/0023-rate-limiting-strategy.md` · `analytics/prompt-stage2.md` S2-P23 · `fuzz/README.md`
