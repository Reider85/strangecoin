# THREAT_MODEL.md — Strangecoin Threat Model (STRIDE)

**Версия:** 3.2 (S2-P23)
**Дата:** 2026-10-29
**Статус:** Stage 2 — актуализирован в S2-P23 (Stage 1.5 — S1.5-P18; Stage 1 — S1-P21; Stage 0 — D02)
**Источник:** `analytics/ARCHITECT3.md` §6 (25 векторов атак) + `analytics/retro-stage0.md` (§4.3, §4.4, §8.2) + `analytics/prompt-stage1.md` S1-P21 (векторы Stage 1) + `analytics/prompt-stage1.5.md` S1.5-P18 (V-18..V-21 митигации Stage 1.5) + `analytics/prompt-stage2.md` S2-P23 (V-1/V-2/V-14/V-15/V-16/V-17/V-24 митигации Stage 2)
**Связанные документы:** `INCIDENT_RESPONSE.md`, `ARCHITECT3.md`, `ROADMAP3.md`, `docs/stage1/INVARIANTS_ENFORCED.md`, `docs/stage1.5/INVARIANTS_ENFORCED.md`, `docs/stage1.5/THREAT_MODEL_STAGE1_5.md`, `docs/stage2/INVARIANTS_ENFORCED.md`, `docs/stage2/THREAT_MODEL_STAGE2.md`, `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`, `fuzz/README.md`

---

## 1. Введение

### 1.1 Scope

Настоящий документ описывает threat model для Strangecoin (Stage 0 — v1.0.0, Stage 1 — v1.1.0-stage1, Stage 1.5 — WASM smart contracts).
Анализ охватывает следующие подсистемы:

**Stage 0:**
- **Консенсус:** PoW валидация, difficulty retargeting, genesis, emission
- **Криптография:** secp256k1 подписи, blake3 хеши, каноническая сериализация
- **Сеть (P2P):** TCP соединения, gossip, sync, rate limiting
- **Хранение:** LevelDB, keystore (AES-256-GCM + PBKDF2)
- **Mempool:** валидация, eviction, лимиты
- **Кошелёк:** keystore шифрование, sign/verify
- **Сборка:** CI/CD, reproducible builds, signatures

**Stage 1 (добавлено в S1-P21):**
- **strangecoin-core:** serialize, consensus, state, economics, governance — 0 I/O
- **Заголовок блока:** `state_root` (Verkle Trie), `tx_root` (merkle), `consensus_version`
- **StateWitness:** генерация/проверка stateless-валидации (API ядра)
- **Headers-first sync:** GET_HEADERS/HEADERS/GET_BLOCKS/BLOCKS, выбор вершины по cumulative work
- **SyncEngine:** inbox pattern, разрыв цикла network↔blockchain
- **Mempool RBF:** feerate, find_replaceable, лимиты замен
- **network_id:** genesis + HELLO, изоляция сетей (mainnet=1, testnet=2, regtest=3)
- **bech32-адреса:** HRP sc1/tsc1/rsc1 с контрольной суммой
- **EventBus:** crossbeam multi-subscriber (BlockApplied, TxRejected, …)
- **governance:** SCIP skeleton + activation height (инвариант №21)
- **tokio:** async-рантайм для новых подсистем

**Stage 1.5 (добавлено в S1.5-P18):**
- **strangecoin-vm-wasm:** WasmiExecutor (wasmi 2.0), instruction-level gas metering (ADR-0014)
- **Tx types:** transfer / create / call (S1.5-P08); account model EOA + Contract (S1.5-P07)
- **Host functions:** 13 whitelisted (crypto/storage/calls/context/logs, S1.5-P10)
- **Precompiles:** 7 адресов 0x01-0x07 (ecrecover/sha256/keccak/blake3/ed25519/bls NotImplemented/modexp, ADR-0015)
- **Reentrancy guard + limits:** `non_reentrant` ABI flag, MAX_CODE_SIZE/MAX_STORAGE_PER_CONTRACT/MAX_CALL_DEPTH/MAX_MEMORY_PAGES/MAX_STACK_HEIGHT (S1.5-P13)
- **Events/Receipts:** `Receipt { gas_used, logs, status }` на каждый tx, receipts sub-trie (S1.5-P14, инвариант #18)
- **Contract storage:** SMT sub-trie per contract, `storage_root` в account leaf, block gas limit (S1.5-P15, инвариант #19 extension / #15)
- **Fuzzing:** host_functions / gas_metering / storage_trie / differential wasmi-vs-wasmtime (S1.5-P16)

**Stage 2 (добавлено в S2-P23):**
- **strangecoin-net:** network runtime вынесен в отдельный крейт `crates/strangecoin-net/` (strangler, S2-P05/S2-P06)
- **Noise Protocol Framework:** XX handshake (snow crate), ChaCha20-Poly1305 transport, monotonic nonces anti-replay, forward secrecy (S2-P07/S2-P08; ADR-0020)
- **Ed25519 node identity:** internal P2P auth (`NodeId = blake3(pubkey)`, auth blob в handshake rounds 2/3; S2-P09) — segregated от secp256k1 wallet keys
- **HELLO capabilities:** `MIN_PROTO_VERSION=2`, network_id + capability bitfield (S2-P10)
- **Peer discovery:** hard-coded seed IPs + DNSSEC validation (hickory-resolver) + ADDR gossip, multi-source bootstrap (S2-P11; V-16)
- **Peer management:** peer_store / peer_manager / connection_pool split, per-IP-range Sybil cap, ban state machine + persistent bans в network.json (S2-P12/S2-P17; ADR-0022)
- **Gossip:** INV / GET_DATA / TX / BLOCK announce + broadcast, per-peer dedup (S2-P13)
- **Compact blocks:** BIP 152 siphash short IDs + fill-up из mempool (S2-P14)
- **Erlay:** mincut reconciliation + fallback full relay (S2-P15; ADR-0021)
- **Rate limiting:** token bucket per peer + per-message caps + ban threshold, wiring в connection loop (S2-P16; ADR-0023)
- **Peer scoring:** weighted 4-factor formula, warn → temp ban → persistent ban ladder (S2-P17)
- **Cleanup/async:** deprecated v0.8.6 messages удалены (S2-P18); network workers на `tokio::spawn` (S2-P19)
- **Shadow tooling:** replay + consensus divergence detector (S2-P20)
- **Fuzzing + интеграционные тесты:** 4 network fuzz targets (S2-P21); `tests/net_*.rs` (S2-P22)

### 1.2 Assumptions

1. **Сеть:** асинхронная, доставка сообщений не гарантирована, задержки переменны
2. **Узлы:** некоторые узлы могут быть враждебными (Byzantine fault tolerant)
3. **Каналы:** TCP соединения перехватываются (plaintext на Stage 0/1/1.5; **Noise Protocol XX + ChaCha20-Poly1305 с Stage 2** — S2-P07/S2-P08; residual: первое соединение к unpinned peer — TOFU, pinning через node_id)
4. **Время:** системные часы узлов могут быть смещены (±几小时)
5. **Хранилище:** локальный диск может быть скомпрометирован (physical access)
6. **Адаптер:** обладает вычислительными ресурсами <50% от общей мощности сети

### 1.3 Trust Boundaries

```
┌─────────────────────────────────────────────────────────┐
│                    Trust Boundary 1                      │
│              (Local Node — Full Trust)                   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │ Wallet   │  │ Consensus│  │ Storage  │              │
│  │ (keys)   │  │ (rules)  │  │ (LevelDB)│              │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘              │
│       │              │              │                    │
│       └──────────────┼──────────────┘                    │
│                      │                                  │
├──────────────────────┼──────────────────────────────────┤
│                      │ Trust Boundary 2                 │
│              (P2P Network — Distrust)                   │
│                      │                                  │
│  ┌──────────┐  ┌─────┴────┐  ┌──────────┐              │
│  │ Peer 1   │  │ Peer 2   │  │ Peer N   │              │
│  │ (?)      │  │ (?)      │  │ (?)      │              │
│  └──────────┘  └──────────┘  └──────────┘              │
│                                                         │
├─────────────────────────────────────────────────────────┤
│                    Trust Boundary 3                      │
│            (External — Zero Trust)                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │ Internet │  │ DNS      │  │ Build CI │              │
│  │          │  │ Seeds    │  │          │              │
│  └──────────┘  └──────────┘  └──────────┘              │
└─────────────────────────────────────────────────────────┘
```

**Trust Boundary 1 (Local Node):** Все компоненты внутри узла считаются доверенными.
Keystore зашифрован паролем пользователя. Consensus rules严格执行.

**Trust Boundary 2 (P2P Network):** Пиры могут быть враждебными. Все входящие
сообщения валидируются ДО обработки. Rate limiting и ban policy.

**Trust Boundary 3 (External):** Internet, DNS, Build CI — полностью недоверены.
Reproducible builds + cosign signatures для верификации бинарников.

---

## 2. STRIDE Categories

| Категория | Описание | Пример в Strangecoin |
|-----------|----------|---------------------|
| **Spoofing** | Подмена_identity (клиент, узел, ключ) | Eclipse attack, Sybil, replay cross-chain |
| **Tampering** | Несанкционированное изменение данных | Time-warp, nothing-at-stake, code injection |
| **Repudiation** | Отрицание совершения действия | Double-spend без доказательства, censorship |
| **Information Disclosure** | Утечка конфиденциальной информации | MEV frontrunning, key leakage, network sniffing |
| **Denial of Service** | Отказ в обслуживании | OOM attack, spam txs, invalid blocks |
| **Elevation of Privilege** | Получение неправомерных полномочий | 51% attack, validator collusion, key compromise |

---

## 3. Threat Vector Table

### 3.1 Network & Peer Attacks

---

#### V-01: Eclipse Attack

| Поле | Значение |
|------|----------|
| **ID** | V-01 |
| **Название** | Eclipse Attack |
| **STRIDE** | Spoofing / Information Disclosure |
| **Описание** | Изоляция целевого узла от честной сети. Атакующий контролирует все входящие и исходящие соединения узла, что позволяет: (1) показывать ложную цепочку, (2) предотвращать получение актуальных блоков, (3) melakukan double-spend через отставшую цепочку. |
| **Stage 0 Mitigation** | Peer discovery через seed nodes + addr gossip; min 8 outgoing + 8 incoming connections; peer diversity (разные подсети); `peer_manager` scoring. Реализовано в P13 (rate limiting), P12 (size limits). |
| **Stage 1 Amendment (BUG-S1-005, 2026-10-11)** | Per-message rate limiting enforced: каждый входящий message в `handle_connection` request-loop проходит `RateLimiter::check_class` (total + per-class bucket: GetHeaders/GetBlocks=100, ChainGossip/Other=30 за 10 s); превышение → бан + закрытие соединения. Ранее check был только на уровне TCP-коннекта — long-lived peer мог стримить неограниченный поток сообщений. |
| **Stage 2 Amendment (S2-P23, 2026-10-29)** | **Усилен.** (1) Multi-source bootstrap: ≥3 источников параллельно (hard-coded seed IPs + DNSSEC-validated DNS + ADDR gossip), union ≥8 peers за ≤30s (`discovery/{mod,seeds,dnssec,addr_gossip}.rs`, S2-P11) — eclipse через подмену одного источника больше не изолирует узел. (2) Peer scoring (ADR-0022): weighted 4-factor formula + ban ladder (S2-P17) — враждебные пиры деградируют и банятся. (3) Per-IP-range connection cap (IPv4 /24, IPv6 /64, `DEFAULT_MAX_PER_IP_RANGE=8`, `peer/pool.rs::count_by_ip_range`, S2-P12) — attacker не может занять все слоты одного диапазона. (4) Noise XX + Ed25519 node_id auth (S2-P07/S2-P09) — подменённый peer не может притвориться доверенным node_id (MITM → NodeId mismatch → drop). |
| **Stage** | P13 (Stage 0/1); S2-P11/S2-P12/S2-P17 (Stage 2) |
| **Residual Risk** | Низкий-средний. Ручная ротация seed-адресов (автоматическая rotation — отложена); per-IP limits неидеальны (NAT/VPN/Tor); unpinned first contact — TOFU (pinning по node_id после первого успеха). |
| **Monitoring** | Мониторинг количества активных пиров; alert при падении ниже 8 outgoing; логирование новых/потерянных соединений; ban-rate per peer; handshake-failure rate (Stage 2). |
| **Код** | `crates/strangecoin-net/src/discovery/{mod,seeds,dnssec,addr_gossip}.rs`, `peer/pool.rs` (per-IP cap), `peer/scoring.rs` + `peer/manager.rs` (ban ladder), `noise/handshake.rs` (auth) |
| **Тест** | `tests/net_peer_scoring.rs` (3: invalid-block ban, latency false-positive, persistent ban restart); `tests/net_helpers.rs` (multi-source bootstrap); `tests/net_noise_handshake.rs` (MITM drop); regression: `tests/rate_limit.rs`, `tests/network.rs` |

---

#### V-02: Partition Attack

| Поле | Значение |
|------|----------|
| **ID** | V-02 |
| **Название** | Partition Attack |
| **STRIDE** | Denial of Service |
| **Описание** | Разделение сети на несколько изолированных частей. Атакующий перехватывает или блокирует сообщения между группами узлов, создавая два или более изолированных консенсуса. |
| **Stage 0 Mitigation** | Cross-checking chain tips с multiple peers; `sync_engine` запрашивает headers у ≥3 пиров; alert при расхождении chain tip > 1 блока. |
| **Stage 2 Amendment (S2-P23, 2026-10-29)** | **Усилен (regression).** sync_engine ≥3 peers — без изменений (S2-P06 миграция в strangecoin-net без behavioral changes; regression `tests/sync_engine.rs` + `tests/sync_headers.rs`). Noise-транспорт (S2-P08) убирает plaintext MITM как инструмент принудительного partition: подмена/ретрай сообщений между сегментами требует possession Ed25519 node_id обеих сторон (V-24). Headers-first sync поверх encrypted сессий (S2-P19). |
| **Stage** | P13; S2-P19 (wire) |
| **Residual Risk** | Низкий-средний. Физический partition (BGP/сетевые сегментации) остаётся вне протокольного контроля; централизация seed nodes — residual (см. V-01 Stage 2). |
| **Monitoring** | Мониторинг chain tip divergence между пирами; alert при расхождении > 1; мониторинг seed node availability. |

---

#### V-03: Selfish Mining

| Поле | Значение |
|------|----------|
| **ID** | V-03 |
| **Название** | Selfish Mining |
| **STRIDE** | Information Disclosure / Elevation of Privilege |
| **Описание** | Майнер скрывает найденные блоки, публикуя их только когда другой майнер найдёт блок на той же высоте. Это позволяет: (1) увеличить долю наград, (2) выбрасывать блоки конкурентов, (3) потенциально melakukan double-spend через reorg. |
| **Stage 0 Mitigation** | `median_time_past` + cumulative work; мониторинг stale rate; alert при аномалиях (>15% stale blocks). В PoS-фазе — slashing за withholding. |
| **Stage** | Мониторинг |
| **Residual Risk** | Высокий. На Stage 0 нет finality gadget (PoS), нет slashing. Selfish mining remains profitable при >33% хешрейта. |
| **Monitoring** | Мониторинг orphan/stale block rate; alert при orphan rate > 10%; мониторинг large miner concentration (pools). |

---

#### V-04: Time-Warp Attack

| Поле | Значение |
|------|----------|
| **ID** | V-04 |
| **Название** | Time-Warp Attack |
| **STRIDE** | Tampering |
| **Описание** | Манипуляция timestamp блока для: (1) ускорения retargeting (снижение difficulty), (2) обхода MTP validation, (3) создания «warps» в timestamps для экстремальных изменений difficulty. |
| **Stage 0 Mitigation** | `median_time_past` с окном 11 блоков (P09); запрет на timestamp > now + 2 часа (P09); retargeting по реальному времени блоков (P08); clamp factor 4x (P08). |
| **Stage** | P08, P09 |
| **Residual Risk** | Низкий. MTP + max future time + clamp adequately mitigate time-warp. Остаётся minor risk: майнер может манипулировать timestamp в пределах MTP..now+2h. |
| **Monitoring** | Мониторинг timestamp drift между блоками; alert при timestamp < MTP или > now + 1h. |

---

### 3.2 Consensus & PoW Attacks

---

#### V-05: 51% Attack (PoW)

| Поле | Значение |
|------|----------|
| **ID** | V-05 |
| **Название** | 51% Attack (Double Spend) |
| **STRIDE** | Elevation of Privilege |
| **Описание** | Атакующий контролирует >50% хешрейта сети и может: (1) melakukan double-spend через reorg, (2) цenzorить транзакции, (3) предотвращать валидацию блоков других майнеров. |
| **Stage 0 Mitigation** | Monitoring of large miner concentration; alert при 51% threshold; checkpointing (weak subjectivity) для new nodes. Финальность — только через chain length (PoW). |
| **Stage** | Мониторинг |
| **Residual Risk** | Высокий (для маленьких сетей). На Stage 0 сеть маленькая, 51% attack成本低. PoS-фаза (Stage 7) решает через slashing. |
| **Monitoring** | Мониторинг hash rate distribution; alert при单一矿工 > 33%; мониторинг reorg depth > 3. |

---

#### V-06: Long-Range Attack (PoS)

| Поле | Значение |
|------|----------|
| **ID** | V-06 |
| **Название** | Long-Range Attack |
| **STRIDE** | Spoofing |
| **Описание** | Атакующий с старыми ключами (которые больше не в validator set) создает альтернативную цепочку с нуля. Поскольку stake уже не locked, nothing-at-stake не deterrent. |
| **Stage 0 Mitigation** | Weak subjectivity checkpoint (синхронизация с trusted source раз в N блоков); validator set rotation каждый epoch; slashing для nothing-at-stake. **Deferred: PoS активируется на Stage 7.** |
| **Stage** | Stage 7 |
| **Residual Risk** | N/A на Stage 0 (PoW-only). Релевантен только после PoS migration. |
| **Monitoring** | N/A на Stage 0. |

---

#### V-07: Nothing-at-Stake (PoS)

| Поле | Значение |
|------|----------|
| **ID** | V-07 |
| **Название** | Nothing-at-Stake |
| **STRIDE** | Tampering |
| **Описание** | В PoS-системе validators подписывают блоки на нескольких форках одновременно (без затрат, в отличие от PoW). Это делает finality неопределенным и позволяет long-range attacks. |
| **Stage 0 Mitigation** | Slashing condition: double-vote → loss of stake; mandatory slashing enforcement в consensus. **Deferred: PoS активируется на Stage 7.** |
| **Stage** | Stage 7 |
| **Residual Risk** | N/A на Stage 0 (PoW-only). |
| **Monitoring** | N/A на Stage 0. |

---

### 3.3 MEV (Maximal Extractable Value)

---

#### V-08: MEV Frontrunning

| Поле | Значение |
|------|----------|
| **ID** | V-08 |
| **Название** | MEV: Frontrunning |
| **STRIDE** | Information Disclosure / Elevation of Privilege |
| **Описание** | Майнер/validator видит pending tx в mempool и вставляет свою tx перед жертвой (например, DEX trade). Это: (1) извлекает value из tx жертвы, (2) создает unfair market. |
| **Stage 0 Mitigation** | Threshold encryption mempool (Stage 5): tx зашифрованы до включения в блок; commitment scheme. **Deferred: MEV mitigation на Stage 5.** |
| **Stage** | Stage 5 |
| **Residual Risk** | Высокий на Stage 0. Mempool открыт, майнеры могут видеть все pending tx. MEV extraction возможен. |
| **Monitoring** | Мониторинг timing tx в блоках; alert при подозрительных patterns (tx перед крупными trades). |

---

#### V-09: MEV Censoring

| Поле | Значение |
|------|----------|
| **ID** | V-09 |
| **Название** | MEV: Censoring |
| **STRIDE** | Denial of Service |
| **Описание** | Майнер/validator намеренно не включает определенные tx в блоки (censorship). Это нарушает liveness и может использоваться для: (1) предотвращения транзакций конкурентов, (2) извлечения MEV через censoring attack. |
| **Stage 0 Mitigation** | `mempool.broadcast` (gossip всем пирам); inclusion lists (PoS-фаза); monitoring of censoring rate. **Deferred: inclusion lists на Stage 7.** |
| **Stage** | Stage 7 (inclusion lists) |
| **Residual Risk** | Средний. На Stage 0 нет принудительных inclusion lists. Майнер может censorить tx без Detection. |
| **Monitoring** | Мониторинг time-to-inclusion для tx; alert при tx waiting > 10 blocks. |

---

#### V-10: MEV Sandwich Attack

| Поле | Значение |
|------|----------|
| **ID** | V-10 |
| **Название** | MEV: Sandwich Attack |
| **STRIDE** | Information Disclosure / Elevation of Privilege |
| **Описание** | Атакующий: (1) places buy tx перед жертвой, (2) жертва выполняет trade (цена растет), (3) атакующий places sell tx после жертвы. Чистый profit за счет price impact trade жертвы. |
| **Stage 0 Mitigation** | Threshold encryption (Stage 5); batch auctions (Stage 5+); user-side slippage protection. **Deferred: MEV mitigation на Stage 5.** |
| **Stage** | Stage 5 |
| **Residual Risk** | Высокий на Stage 0. Sandwich attacks возможны через mempool inspection. |
| **Monitoring** | Мониторинг sandwich patterns в mempool; alert при подозрительных tx sequences. |

---

### 3.4 Replay & Authentication Attacks

---

#### V-11: Cross-Chain Replay Attack

| Поле | Значение |
|------|----------|
| **ID** | V-11 |
| **Название** | Replay Attack (Cross-Chain) |
| **STRIDE** | Spoofing |
| **Описание** | Транзакция, валидная в testnet, повторяется в mainnet (или наоборот). Если формат tx одинаковый и нет chain_id, tx может быть replayed. |
| **Stage 0 Mitigation** | `chain_id` в каждой tx (P05); mainnet=1, testnet=2, regtest=3; инвариант #10: `tx.chain_id != node chain_id → reject`. |
| **Stage 1 Mitigation** | **BUG-S1-004 (2026-10-11):** `current_chain_id()` (хардкод regtest) удалён; chain_id пробрасывается из `Config.network_id` через `Blockchain::chain_id` → `Mempool::insert`, `BlockView.chain_id` → `validate_and_apply` (per-tx reject) и core `apply_block` (state-machine gate + эмиссионное расписание). Межсетевой генезис: `validate_genesis(block, chain_id)` против network-specific `EXPECTED_*_GENESIS_HASH`; тесты `tests/chain_id.rs`, `tests/genesis_key.rs`. |
| **Stage** | P05 |
| **Residual Risk** | Низкий. chain_id полностью mitigates cross-chain replay. |
| **Monitoring** | Alert при reject из-за chain_id mismatch (потенциальная атака или misconfiguration). |

---

#### V-12: Intra-Chain Replay Attack

| Поле | Значение |
|------|----------|
| **ID** | V-12 |
| **Название** | Replay Attack (Intra-Chain) |
| **STRIDE** | Spoofing |
| **Описание** | Повторение уже выполненной транзакции. Атакующий повторно отправляет signed tx, надеясь что receiver получит средства дважды. |
| **Stage 0 Mitigation** | `account.nonce` строго инкрементируется (P05); инвариант #11: `tx.nonce != account.nonce + 1 → reject`; txid = commitment (P07). |
| **Stage** | P05, P07 |
| **Residual Risk** | Низкий. Nonce enforcement + txid uniqueness полностью mitigates replay. |
| **Monitoring** | Alert при reject из-за nonce mismatch; мониторинг nonce gaps. |

---

### 3.5 Denial of Service Attacks

---

#### V-13: DoS OOM Attack

| Поле | Значение |
|------|----------|
| **ID** | V-13 |
| **Название** | DoS: OOM (Out of Memory) |
| **STRIDE** | Denial of Service |
| **Описание** | Атакующий отправляет сообщение с огромным length field (например, 10GB). Если `vec![0; length]` аллоцируется ДО проверки, узел потребляет всю память и падает (OOM kill). |
| **Stage 0 Mitigation** | Length-prefixed framing с проверкой ДО аллокации (P12); `MAX_MESSAGE_SIZE = 32MB`, `MAX_BLOCK_SIZE = 4MB`, `MAX_TX_SIZE = 256KB`; инвариант #16. **BUG-S1-006 (2026-10-11):** те же `MAX_BLOCK_SIZE`/`MAX_TX_SIZE` enforced в `block_executor::validate_and_apply` первыми проверками — validation-layer путь (`adopt_candidate`/`commit_block`/`rebuild_from_chain`) больше не обходит wire-лимиты (инвариант #7). |
| **Stage** | P12 |
| **Residual Risk** | Низкий. P12 полностью mitigates OOM через pre-allocation validation. |
| **Monitoring** | Alert при reject из-за SizeLimitExceeded; мониторинг rejected message rate. |

---

#### V-14: DoS Spam Txs

| Поле | Значение |
|------|----------|
| **ID** | V-14 |
| **Название** | DoS: Spam Transactions |
| **STRIDE** | Denial of Service |
| **Описание** | Атакующий отправляет миллионы мусорных транзакций, заполняя mempool и перегружая валидацию. Это предотвращает включение легитимных tx в блоки. |
| **Stage 0 Mitigation** | `mempool` rate limit per sender (P13); `min_relay_fee` (Stage 5); `mempool.eviction` по feerate (P14); MAX_PENDING_TXS limit (P14). |
| **Stage 2 Amendment (S2-P23, 2026-10-29)** | **Усилен.** Token bucket per peer + per-message-type caps (ADR-0023, S2-P16): main bucket 1000/100 tok/s + саб-бакеты (INV 100/10s, BLOCK 30/10s, TX 1000/10s); violation window (5 за 60s) → бан; production wiring в `peer/connection_loop.rs` (`check_rate` → `apply_rate_verdict` → `record_rate_violation` + `ban_peer`, persistent в network.json). Gossip-announced TX проходят тот же mempool insert (S2-P13) — спам-INV/TX фильтруется до validation. Интеграция со peer scoring (ADR-0022): rate violations снижают score → ban ladder. |
| **Stage** | P13, P14; S2-P13/S2-P16/S2-P17 (Stage 2) |
| **Residual Risk** | Низкий. Mempool cap + rate limiting + scoring adequately mitigate spam; fee=0 (до Stage 5) оставляет дешёвый spam при наличии PoW-доступа — rate limit на relay-слойе остаётся первичной линией. |
| **Monitoring** | Мониторинг mempool size; alert при > 80% capacity; rejected tx rate per peer; rate-limit ban rate (Stage 2). |
| **Код** | `crates/strangecoin-net/src/rate_limiter.rs`, `peer/connection_loop.rs:216-244`, `peer/manager.rs::apply_rate_verdict` |
| **Тест** | `tests/net_rate_limit_integration.rs::spammer_1000_inv_per_second_banned_after_five_violations_and_persists`, `::legitimate_peer_under_100_inv_per_10s_never_banned`; `rate_limiter.rs` unit; regression `tests/two_clients.rs`, `tests/rbf.rs` |

---

#### V-15: DoS Invalid Blocks

| Поле | Значение |
|------|----------|
| **ID** | V-15 |
| **Название** | DoS: Invalid Blocks |
| **STRIDE** | Denial of Service |
| **Описание** | Пиров атакует invalid блоками. Каждый invalid block требует expensive validation (signature verification, PoW check). Это перегружает CPU узла. |
| **Stage 0 Mitigation** | `peer_manager` scoring; ban после N invalid blocks; `chain_selector` не применяет блоки до валидации (P13). |
| **Stage 2 Amendment (S2-P23, 2026-10-29)** | **Mitigated.** Peer scoring (ADR-0022, S2-P17): `invalid_blocks_txs_sent` — доминирующий фактор (weight 0.5); weighted score < threshold → warn → temp ban 1h → persistent ban 30d; `PeerManager::record_invalid_block` + `tick_at` ban expiry; persistent баны переживают рестарт (`network.json: banned_peers`, ADR-0022 schema). Rate limiting (S2-P16) дополнительно ограничивает поток invalid-блоков до validation. Block/tx size caps (инвариант #7) + Noise framing (инвариант #16 ext) — pre-allocation reject. |
| **Stage** | P13; S2-P16/S2-P17 (Stage 2) |
| **Residual Risk** | Низкий. Ban-after-N + scoring adequately mitigate invalid block spam; false-positive path (high latency but valid) — не банится (`tests/net_peer_scoring.rs::high_latency_but_valid_peer_is_never_banned`). |
| **Monitoring** | Alert при ban; мониторинг invalid block rate per peer; scoring-distribution snapshot. |
| **Код** | `crates/strangecoin-net/src/peer/scoring.rs` (compute_score/decide, ADR-0022), `peer/manager.rs::record_invalid_block:516`, `peer/store.rs::ban_peer` |
| **Тест** | `tests/net_peer_scoring.rs::malicious_peer_banned_after_configured_invalid_block_threshold`, `::persistent_ban_survives_restart_via_network_json`; regression: `tests/block_executor.rs` (reject-матрица), `tests/sync_headers.rs` |

---

### 3.6 Network Poisoning Attacks

---

#### V-16: Eclipse via DNS Poisoning

| Поле | Значение |
|------|----------|
| **ID** | V-16 |
| **Название** | Eclipse via DNS Poisoning |
| **STRIDE** | Spoofing |
| **Описание** | Атакующий отравляет DNS-записи seed nodes, перенаправляя узлы на враждебные ноды. Это подготавливает eclipse attack (V-01). |
| **Stage 0 Mitigation** | Hard-coded seed IPs (not just DNS); DNSSEC validation; rotation of seeds. |
| **Stage 2 Amendment (S2-P23, 2026-10-29)** | **Усилен.** DNSSEC validation реализована (S2-P11): `DnssecResolver` (hickory-resolver `=0.26.3`, `opts.validate`) + `apply_dnssec_policy` — reject не-Secure answers; multi-source bootstrap параллельно опрашивает hard-coded IPs + DNS + ADDR gossip — отравленный DNS-источник один не изолирует узел (union ≥8 peers). |
| **Stage** | P01; S2-P11 (Stage 2) |
| **Residual Risk** | Средний-низкий. Автоматическая rotation seeds не реализована (ручная через network.json); DNSSEC полагается на доверие к root KSI; hard-coded IPs могут устареть. |
| **Monitoring** | Мониторинг seed node availability; alert при неудачных подключениях к seeds; alert на DNSSEC validation failures. |
| **Код** | `crates/strangecoin-net/src/discovery/{seeds,dnssec}.rs` |
| **Тест** | `tests/net_helpers.rs` (multi-source bootstrap determinism); discovery unit-тесты (dnssec policy, bootstrap merges) |

---

#### V-17: Sybil Attack

| Поле | Значение |
|------|----------|
| **ID** | V-17 |
| **Название** | Sybil Attack |
| **STRIDE** | Spoofing |
| **Описание** | Атакующий создает множество фейковых пиров для: (1) eclipse attack, (2) увеличения влияния на gossip, (3) перегрузки peer management. |
| **Stage 0 Mitigation** | `peer_manager` scoring; limit on connections per IP range; ban on protocol violations (P13); connection limits (max_peers config). |
| **Stage 1 Amendment (BUG-S1-005)** | Per-message ban усиливает Sybil-митигацию: сообщения каждого соединения ограничены total + per-class bucket'ами; спам-соединение банится по message rate, а не только по protocol violation. Per-IP connection limits по-прежнему gap (peer_manager не реализован). |
| **Stage 2 Amendment (S2-P23, 2026-10-29)** | **Mitigated (gap закрыт).** (1) Connection pool per-IP-range Sybil cap: `ConnectionPool::count_by_ip_range` (IPv4 /24, IPv6 /64), `DEFAULT_MAX_PER_IP_RANGE=8`, config `max_per_ip_range` (S2-P12, `peer/pool.rs:209`); `connect_to_peer`/`accept_inbound` отклоняют при cap. (2) Peer scoring + ban ladder (S2-P17) — сибил-набор быстро деградирует по score. (3) Token bucket + violation ban (S2-P16) — спам-соединения банятся по rate. (4) Ed25519 node_id (S2-P09) — каждый Sybil-узел требует own keypair; auth-fail rate виден в scoring. |
| **Stage** | P13; S2-P12/S2-P16/S2-P17 (Stage 2) |
| **Residual Risk** | Средний-низкий. IP-based limits не идеальны (NAT, VPN, Tor); proof-of-work для connections — отложен (Stage 6+). |
| **Monitoring** | Мониторинг connections per IP range; alert при > max_per_ip_range; Sybil-probe: множество handshake-fail с одного диапазона. |
| **Код** | `crates/strangecoin-net/src/peer/pool.rs` (count_by_ip_range), `peer/manager.rs` (DEFAULT_MAX_PER_IP_RANGE=8), `peer/scoring.rs`, `rate_limiter.rs` |
| **Тест** | `tests/net_peer_scoring.rs` (3); `tests/net_rate_limit_integration.rs` (2); pool unit-тесты (per-IP cap reject) |

---

### 3.7 Smart Contract Attacks

---

#### V-18: Reentrancy

| Поле | Значение |
|------|----------|
| **ID** | V-18 |
| **Название** | Reentrancy (Smart Contracts) |
| **STRIDE** | Tampering |
| **Описание** | Контракт вызывает сам себя (или другой контракт) до завершения текущего execution. Это позволяет: (1)多次withdraw before balance update, (2) обход checks-effects-interactions pattern. |
| **Stage 0 Mitigation** | Дизайн-митигация зафиксирован: `reentrant: bool` в `ExecutionContext`; `nonReentrant` modifier pattern; `gas_limit` enforcement. **Deferred: WASM смарт-контракты на Stage 1.5.** |
| **Stage 1.5 Amendment (S1.5-P13, S1.5-P17)** | **Mitigated.** (1) Host function `call`/`delegatecall`/`staticcall` перед входом в кадр проверяет `state.call_stack.contains(&to)`; если reentry и целевой ABI entry несёт флаг `non_reentrant` → abort `VmError::ReentrancyDetected` (`vm-wasm/src/host/calls/mod.rs:108-140`). (2) Дочерний кадр получает `ctx.reentrant = is_reentrant`. (3) Reentrancy без `non_reentrant` флага легальна и завершается (MVP: guard opt-in per entry point, не глобальный запрет). (4) Depth cap `MAX_CALL_DEPTH` = 1024 ограничивает рекурсию даже без ABI-флага. |
| **Stage** | S1.5-P13 (enforced), S1.5-P17 (e2e) |
| **Residual Risk** | Низкий. Guard opt-in: контракт без `non_reentrant` entry может reenter сам себя — это осознанное поведение (как CALL в Ethereum), защищено только depth/gas. Продакер контракта обязан помечать money-moving entry points. Cross-contract reentrancy закрыта тем же `call_stack`. |
| **Monitoring** | Мониторинг `ReentrancyDetected` reject-ов; spike → потенциальная атака на конкретный контракт или regression guard-а. |
| **Код** | `crates/strangecoin-core/src/abi/mod.rs` (`AbiEntry.non_reentrant`), `core/vm/context.rs` (`reentrant: bool`), `crates/strangecoin-vm-wasm/src/host/calls/mod.rs:108-140` (call_stack + ABI lookup), `core/vm/limits.rs::MAX_CALL_DEPTH` |
| **Тест** | vm-wasm `tests/reentrancy_and_limits.rs::self_call_with_non_reentrant_abi_is_rejected`, `::cross_contract_reentrancy_is_rejected`, `::self_call_with_non_reentrant_false_is_allowed`, `::call_depth_beyond_limit_is_rejected`; e2e `tests/vm_reentrancy_attack.rs::reentrancy_attack_end_to_end_is_rejected`, `::reentrancy_without_non_reentrant_flag_runs_and_terminates` |

---

#### V-19: Integer Overflow (Smart Contracts)

| Поле | Значение |
|------|----------|
| **ID** | V-19 |
| **Название** | Integer Overflow (Smart Contracts) |
| **STRIDE** | Tampering |
| **Описание** | Переполнение в арифметике смарт-контракта. Например, `uint256 balance = 0 - 1` → massive balance. Это позволяет: (1) создать токены из ничего, (2) обойти проверки баланса. |
| **Stage 0 Mitigation** | Дизайн: checked arithmetic (Rust default); `SafeMath`-style precompiles; gas cost for arithmetic ops. **Deferred: WASM смарт-контракты на Stage 1.5.** |
| **Stage 1.5 Amendment (S1.5-P11, S1.5-P13)** | **Mitigated.** (1) Host/native код — Rust checked/saturating arithmetic (нет unchecked переполнений вне wasmi-кадра; см. `storage_write` saturating-adds к cap). (2) Precompile `0x07 modexp` — бóльшая часть целочисленной криптографии выносится в audited dispatcher с checked length-полями (`huge_length_header_is_invalid`). (3) `MAX_CODE_SIZE` = 24 KB (24 * 1024) резон в `verify_transaction` для CreateTx (`consensus.rs:354`) — один deploy не может протащить неограниченный код. (4) i32/wasm-арифметика контракта детерминирована (wrap в wasm — consensus-safe, не host compromise). |
| **Stage** | S1.5-P11 + S1.5-P13 |
| **Residual Risk** | Низкий для host-слоя; medium для бизнес-логики контрактов (trapping-арифметика в wasm — ответственность контракт-автора, как Solidity unchecked). modexp длины ограничены gas-стоимостью. |
| **Monitoring** | Мониторинг Failed-Receipts с переполнением/trap; fuzz-таргеты S1.5-P16 (gas_metering, storage_trie) как regression. |
| **Код** | `core/vm/limits.rs::MAX_CODE_SIZE`, `core/consensus.rs:354` (CreateTx code check), `vm-wasm/src/precompiles/modexp.rs` (+ dispatcher), `vm-wasm/src/host/storage/storage_write.rs` (saturating to cap) |
| **Тест** | vm-wasm `tests/reentrancy_and_limits.rs::create_code_size_at_limit_accepted`, `::create_code_size_over_limit_rejected`; `tests/precompiles.rs::modexp_*` (10: vectors, zero exponent/modulus, padding, truncated/huge header); fuzz `fuzz/fuzz_targets/gas_metering.rs` |

---

#### V-20: Storage Exhaustion

| Поле | Значение |
|------|----------|
| **ID** | V-20 |
| **Название** | Storage Exhaustion |
| **STRIDE** | Denial of Service |
| **Описание** | Контракт заполняет весь state, делая узел недоступным. Каждый write стоит gas, но если gas limit высокий, контракт может заполнить terabytes. |
| **Stage 0 Mitigation** | Дизайн: `max_storage_per_contract`; `state_rent` (Stage 3+); gas cost for storage writes. **Deferred: WASM смарт-контракты на Stage 1.5.** |
| **Stage 1.5 Amendment (S1.5-P13, S1.5-P15)** | **Mitigated (MVP).** (1) `MAX_STORAGE_PER_CONTRACT` = 1 MB (`core/vm/limits.rs:14`); `storage_write` host function считает persisted + staged + new value и отклоняет write сверх cap (`vm-wasm/src/host/storage/storage_write.rs:47-58`) → Failed-Receipt, overlay не flush-ится. (2) Block gas limit 1M (`BLOCK_GAS_LIMIT`) ограничивает количество storage-writes в блоке (каждый write — host-call weight, ADR-0014). (3) Кап per-contract, не global — глобальный рост state ограничен числом контрактов × 1 MB × block-rate. |
| **Stage** | S1.5-P13 + S1.5-P15 |
| **Residual Risk** | Средний. Нет state rent / global state cap: спам-деплои многих контрактов по 1 MB каждый — эвристика rate-limited только gas-ом; state-растение не стягивается (rent — Stage 3+). Storage-undo/reorg — derived, не disk-DoS. |
| **Monitoring** | Мониторинг размера contract_storage map; alert при аномальном росте; Failed-Receipts с storage cap reject. |
| **Код** | `core/vm/limits.rs::MAX_STORAGE_PER_CONTRACT`, `vm-wasm/src/host/storage/storage_write.rs`, `core/state/contract_storage.rs` (overlay flush), `core/vm/gas.rs::BLOCK_GAS_LIMIT` |
| **Тест** | vm-wasm `tests/reentrancy_and_limits.rs::storage_cap_at_limit_accepted`, `::storage_cap_exceeded_rejected`; `tests/contract_storage.rs` (write > 1 MB → Failed, без flush); `tests/vm_gas_exhaustion.rs` (block gas ceiling); fuzz `fuzz/fuzz_targets/storage_trie.rs` |

---

#### V-21: Code Injection (Malicious WASM)

| Поле | Значение |
|------|----------|
| **ID** | V-21 |
| **Название** | Code Injection (Malicious WASM) |
| **STRIDE** | Tampering / Elevation of Privilege |
| **Описание** | Загрузка вредоносного WASM-модуля, который: (1) выполняет произвольный код, (2) читает/пишет state, (3) обходит gas metering. |
| **Stage 0 Mitigation** | Дизайн: wasmi sandbox (no I/O by default); host functions whitelist; gas metering prevents infinite loops. **Deferred: WASM смарт-контракты на Stage 1.5.** |
| **Stage 1.5 Amendment (S1.5-P06, S1.5-P09, S1.5-P10, S1.5-P16)** | **Mitigated.** (1) wasmi 2.0 sandbox: без fs/net/env по умолчанию; линейная память ограничена `MAX_MEMORY_PAGES` = 16 (1 MiB) через StoreLimits; value stack `MAX_STACK_HEIGHT` = 1M байт; runaway recursion → детерминированный trap. (2) Host functions — whitelist из 13 функций (никакого произвольного host access; каждая — explicit import). (3) Gas metering на каждой инструкции + host-call weights (ADR-0014); OutOfGas → детерминированное сожжение полного бюджета, код не исполняется дальше. (4) Deploy-код валидируется (`register_code_rejects_invalid_wasm`), размер ≤ `MAX_CODE_SIZE`. (5) Differential testing wasmi vs wasmtime (S1.5-P16) — детерминизм исполнения. |
| **Stage** | S1.5-P06/P09/P10/P16 |
| **Residual Risk** | Средний-низкий. wasmi — не formally verified; возможны 0-day в самом интерпретаторе (см. fuzz-таргеты S1.5-P16 как continuous regression). Бизнес-логика контрактов (кража средств по ABI) — вне sandbox-митигаций (audit контрактов — экосистема Stage 4+). |
| **Monitoring** | Мониторинг trap/OutOfGas rate per contract; fuzz CI (`fuzz-canonical-decode` + vm-таргеты); alert на wasmi panics (не должны случаться — crash = security bug). |
| **Код** | `crates/strangecoin-vm-wasm/src/lib.rs` (WasmiExecutor, StoreLimits, engine config), `core/vm/limits.rs`, `vm-wasm/src/host/mod.rs` (13 imports), `core/consensus.rs:354` (MAX_CODE_SIZE), `vm-wasm/gas.rs` (instruction metering) |
| **Тест** | vm-wasm `tests/reentrancy_and_limits.rs::memory_declaration_over_limit_rejected`, `::memory_grow_beyond_limit_traps_out_of_memory`, `::runaway_recursion_hits_stack_limit`, `::call_depth_beyond_limit_is_rejected`; `tests/events_and_receipts.rs::register_code_rejects_invalid_wasm`; `tests/gas_metering.rs`; fuzz `fuzz/fuzz_targets/{host_functions,differential_wasmi_vs_wasmtime}.rs` |

---

### 3.8 Build & Supply Chain Attacks

---

#### V-22: Compromised Build

| Поле | Значение |
|------|----------|
| **ID** | V-22 |
| **Название** | Compromised Build |
| **STRIDE** | Tampering |
| **Описание** | Зловредный binary, скомпрометированный на этапе сборки. Атакующий: (1) подменяет исходники в repo, (2) inject backdoor в CI, (3) публикует compromised binary как «официальный». |
| **Stage 0 Mitigation** | Reproducible builds (P24); cosign/sigstore signatures; SLSA provenance; deterministic build flags (`RUSTFLAGS="--remap-path-prefix"`). |
| **Stage** | P24 |
| **Residual Risk** | Низкий. Reproducible builds + cosign adequately mitigate build compromise. |
| **Monitoring** | Мониторинг CI pipeline failures; alert при unexpected build outputs. |

---

#### V-23: Key Compromise

| Поле | Значение |
|------|----------|
| **ID** | V-23 |
| **Название** | Key Compromise |
| **STRIDE** | Elevation of Privilege |
| **Описание** | Украденный private key позволяет: (1) подписывать транзакции от имени жертвы, (2) контролировать funds, (3) potential double-spend через reorg с скомпрометированным ключом. |
| **Stage 0 Mitigation** | Keystore AES-256-GCM + PBKDF2 (≥210k iters) (P04); hardware wallet support (Ledger) — Stage 2. Account Abstraction (social recovery, multisig) — Stage 5. |
| **Stage** | P04 (keystore), Stage 5 (AA), Stage 2 (Ledger) |
| **Residual Risk** | Средний. На Stage 0 keystore зашифрован, но: (1) пароль может быть weak, (2) нет social recovery, (3) нет hardware wallet. |
| **Monitoring** | Мониторинг unusual tx patterns (large amounts, new addresses); alert при tx с нового адреса. |

---

#### V-24: Network MITM

| Поле | Значение |
|------|----------|
| **ID** | V-24 |
| **Название** | Network MITM (Man-in-the-Middle) |
| **STRIDE** | Spoofing / Information Disclosure |
| **Описание** | Перехват TCP трафика между узлами. Атакующий может: (1) читать все сообщения (gossip, tx, blocks), (2) модифицировать сообщения, (3) подменять пиры. |
| **Stage 0 Mitigation** | Noise Protocol Framework (Stage 2) — handshake XX, ephemeral keys, forward secrecy. **На Stage 0: plaintext TCP, нет encryption.** |
| **Stage 2 Amendment (S2-P23, 2026-10-29)** | **Mitigated (S2-P07/S2-P08/S2-P09, ADR-0020).** (1) Noise XX handshake (`snow =0.10.0`): mutual auth без PKI, 3 round trips, static X25519-ключи; эфемерные ключи — forward secrecy (`noise/handshake.rs`; детерминированный вывод из long-term seed через HKDF — `noise/ephemeral.rs`, чек-лист детерминизма). (2) Transport: ChaCha20-Poly1305 AEAD framing (`encrypt_frame`/`decrypt_frame`, `MAX_MESSAGE_SIZE` до аллокации), monotonic nonces + `NoiseError::Replay` anti-replay, rekey primitive (`noise/transport.rs`). (3) Ed25519 node identity: `NodeId = blake3(pubkey)`, auth blob `[pubkey||sig]` в rounds 2/3, `into_transport → (NoiseTransport, NodeId)` (`noise/auth.rs`); MITM со своими ключами → NodeId mismatch → drop. (4) Единый транспорт = Noise (S2-P19): plaintext-путь монолита удалён — Stage 1.5 plaintext-клиенты несовместимы до тега `v2.0.0-stage2`. |
| **Stage** | Stage 2 (S2-P07, S2-P08, S2-P09, S2-P19) |
| **Residual Risk** | Низкий. (1) Unpinned first contact — TOFU: unpinned handshake принимается как unverified (`tests/net_noise_handshake.rs::unpinned_first_contact_is_accepted_as_unverified`), pinning — по node_id после первого успеха; opportunistic MITM на первой встрече возможен (как Bitcoin bloom-era peer exchange). (2) Ephemeral-ключи выводятся из long-term seed (reproducibility, §Determinism) — компрометация seed раскрывает записанные сессии этого узла (trade-off зафиксирован S2-P09). (3) snow — не formally verified; version pinned (`=0.10.0`). |
| **Monitoring** | Handshake-failure rate; NodeId mismatch drops; unexpected disconnects; alert на Replay-детекты (potential replay attack or nonce desync). |
| **Код** | `crates/strangecoin-net/src/noise/{handshake,transport,auth,ephemeral}.rs`, `protocol.rs::encrypt_frame:427`/`decrypt_frame:471`, `peer/manager.rs::connect_to_peer`/`accept_inbound` |
| **Тест** | `tests/net_noise_handshake.rs` (4: two-node roundtrip, `mitm_with_own_keys_is_dropped_on_node_id_mismatch`, unpinned-TOFU, transport pair); crate unit-тесты handshake/transport/auth; fuzz `fuzz/fuzz_targets/noise_handshake.rs` (S2-P21; CI-side) |

---

### 3.9 Validator Attacks (PoS)

---

#### V-25: Validator Collusion

| Поле | Значение |
|------|----------|
| **ID** | V-25 |
| **Название** | Validator Collusion |
| **STRIDE** | Elevation of Privilege |
| **Описание** | Validators сговариваются для: (1) double-spend через coordinated reorg, (2) censorship attacks, (3) MEV extraction через collusion. |
| **Stage 0 Mitigation** | Max stake per validator (5% от total); random validator selection; slashing conditions; whistleblower rewards. **Deferred: PoS на Stage 7.** |
| **Stage** | Stage 7 |
| **Residual Risk** | N/A на Stage 0 (PoW-only). |
| **Monitoring** | N/A на Stage 0. |

---

## 4. Additional Stage 0/1-Specific Vectors

Помимо 25 основных векторов из ARCHITECT3.md §6,以下是在 Stage 0/1 специфичные векторы. Подсекция 4.10 добавлена в S1-P21 (векторы поверхностей Stage 1).

---

#### V-26: Testnet Low-Difficulty → 51% Attack

| Поле | Значение |
|------|----------|
| **ID** | V-26 |
| **Название** | Testnet Low-Difficulty Attack |
| **STRIDE** | Elevation of Privilege |
| **Описание** | Testnet использует низкий difficulty для быстрого блока. Атакующий может легко получить >50% хешрейта на testnet и: (1) переписать историю, (2) double-spend testnet funds, (3) потенциально отправить скомпрометированные блоки в mainnet (если chain_id не проверяется). |
| **Stage 0 Mitigation** | chain_id separation (P05: testnet=2, mainnet=1); nodes reject blocks с wrong chain_id; testnet faucet для distribution. |
| **Stage** | P05 |
| **Residual Risk** | Средний (testnet-only). chain_id isolation защищает mainnet от cross-network contamination, но low testnet difficulty позволяет >50% rewrite истории testnet (double-spend testnet funds). **Obligation: testnet difficulty retarget hardening — фиксируется SCIP, activation height post-mainnet-freeze; mainnet — Stage 6 external audit; testnet residual accepted до retarget fix** (закрывает BUG-S0-033). |
| **Monitoring** | Мониторинг chain_id в rejected blocks; alert при аномалиях. |

---

#### V-27: Keystore File Theft

| Поле | Значение |
|------|----------|
| **ID** | V-27 |
| **Название** | Keystore File Theft |
| **STRIDE** | Information Disclosure |
| **Описание** | Физический доступ к machine → кража keystore файла. Если пароль weak, атакующий может brute-force и получить private key. |
| **Stage 0 Mitigation** | Keystore AES-256-GCM + PBKDF2 (≥210k iters) (P04); пароль через env var (P15); never в config. |
| **Stage** | P04, P15 |
| **Residual Risk** | Средний. Если пароль weak, brute-force возможен. Hardware wallet (Stage 2) решает. |
| **Monitoring** | Мониторинг failed decrypt attempts; lockout после N failures. |

---

#### V-28: Config File Leakage

| Поле | Значение |
|------|----------|
| **ID** | V-28 |
| **Название** | Config File Leakage |
| **STRIDE** | Information Disclosure |
| **Описание** | Конфигурационные файлы содержат: (1) network topology, (2) listen addresses, (3) potentially sensitive paths. Утечка помогает атакующему спланировать targeted attack. |
| **Stage 0 Mitigation** | config.toml содержит только non-secret parameters (P15); keystore secrets в отдельных зашифрованных файлах; .gitignore для sensitive files. |
| **Stage** | P15 |
| **Residual Risk** | Низкий. Config не содержит secrets, только operational parameters. |
| **Monitoring** | Мониторинг git operations; alert при commit sensitive files. |

---

#### V-29: Genesis Manipulation

| Поле | Значение |
|------|----------|
| **ID** | V-29 |
| **Название** | Genesis Manipulation |
| **STRIDE** | Tampering |
| **Описание** | Подмена genesis.json или EXPECTED_GENESIS_HASH. Атакующий: (1) меняет genesis block, (2) подменяет initial holder, (3) перезапускает сеть с alternative genesis. |
| **Stage 0 Mitigation** | EXPECTED_GENESIS_HASH в consensus.rs (P10); validate_genesis() проверяет hash при старте; узел отказывается стартовать при mismatch; regtest genesis без hash check. |
| **Stage** | P10 |
| **Residual Risk** | Низкий. Genesis validation adequately mitigates manipulation. |
| **Monitoring** | Alert при GenesisMismatch ( potential attack or misconfiguration). |

---

#### V-30: Nonce Manipulation

| Поле | Значение |
|------|----------|
| **ID** | V-30 |
| **Название** | Nonce Manipulation |
| **STRIDE** | Tampering |
| **Описание** | Попытка отправить tx с nonce, который: (1) уже использован (replay), (2) слишком далеко вперед (gap), (3) не соответствует account state. |
| **Stage 0 Mitigation** | Nonce validation в add_transaction (P05): `tx.nonce != account.nonce + 1 → reject`; mempool nonce tracking (P14). |
| **Stage** | P05, P14 |
| **Residual Risk** | Низкий. Strict nonce enforcement adequately mitigates manipulation. |
| **Monitoring** | Мониторинг rejected tx по nonce; alert при аномалиях. |

---

#### V-31: Genesis Key from Public String

| Поле | Значение |
|------|----------|
| **ID** | V-31 |
| **Название** | Genesis Key Derived from Public String |
| **STRIDE** | Elevation of Privilege / Information Disclosure |
| **Описание** | Приватный ключ генезисного холдера 1 000 000 000 монет выводился хэшированием публичной строки `"strangecoin-genesis-seed-2026"` (функция `genesis_keypair()` в `src/consensus/mod.rs`). Любой, кто видел исходный код/git history, воспроизводил ключ и мог потратить генезисные средства. |
| **Stage 0 Mitigation** | Осознанный компромисс для Stage 0/testnet. Комментарий в коде: «Real offline key will be used before mainnet freeze». На testnet/regtest генезисные средства не имеют реальной ценности. |
| **Mitigation (закрыто S1.5-P01 / BUG-S0-015)** | `genesis_keypair()` и seed-строка **удалены** из `src/consensus/mod.rs`; `genesis.json` содержит только `initial_holder_pubkey`. SCIP-0001 (`docs/SCIP/scip-0001-genesis-key-replacement.md`) фиксирует offline-ключ как обязательный gate до mainnet freeze. Тест `tests/genesis_key.rs`: genesis валидируется по `EXPECTED_GENESIS_HASH` без секрета в коде + regression-guard на seed-строку. |
| **Stage** | Pre-mainnet (SCIP-0001) |
| **Residual Risk** | **Высокий** для любой публичной цепочки, запущенной с текущим testnet-генезисом. Residual: genesis key derived from `"strangecoin-genesis-seed-2026"`; **BURNED** (pubkey остался в testnet `genesis.json`); replacement via **SCIP-0001 mandatory before block 1 of mainnet**. Код больше не хранит секрет; замена ключа — операционный шаг (offline key → новый pubkey → новый `EXPECTED_GENESIS_HASH`). Offline-keygen tooling готов (2026-10-14, BUG-S1-015): `examples/generate_offline_genesis_key.rs` — исполнение ops-gate остаётся human action на air-gapped машине. |
| **Monitoring** | Мониторинг genesis-транзакций в mainnet; alert на любой mainnet-запуск без SCIP-0001-подписи. |

---

#### V-32: Grant Blocks in Consensus Path

| Поле | Значение |
|------|----------|
| **ID** | V-32 |
| **Название** | Grant Blocks Bypass Consensus Rules |
| **STRIDE** | Elevation of Privilege / Tampering |
| **Описание** | Механизм `create_grant_block` (первичная эмиссия: 10000 монет «initial_wallet_address» первому кошельку) — это `is_coinbase: true` без подписи, прямая мутация `balances` в обход mempool и эмиссионных правил. В Stage 0 `validate_chain` содержал захардкоженное исключение `block.index != 1`, а magic-строки `"genesis"`/`"coinbase"` в sender пропускали проверку баланса. |
| **Mitigation (закрыто S1-P01)** | Флаг `allow_grant_blocks` (default = false, true допустим только для regtest/легаси-миграций). При false: блок №1 валидируется по общим правилам (coinbase ≤ block_reward_at_height), magic-строки в sender → reject, grant-вызов → typed error `GrantBlocksDisabled`. Генезис по-прежнему валидируется по `EXPECTED_GENESIS_HASH`. Commit `cb0ac1a`. |
| **Stage** | S1-P01 (закрыт) |
| **Residual Risk** | Низкий. Консенсусный обход исключён при выключенном флаге. Остаток: флаг должен оставаться false на mainnet/testnet — операционная ответственность, не код. Легаси-миграции БД — отдельный путь (не консенсусный). |
| **Monitoring** | Мониторинг coinbase-транзакций с amount > block_reward_at_height; alert; лог GrantBlocksDisabled. |

---

#### V-33: Release Pipeline Never Executed

| Поле | Значение |
|------|----------|
| **ID** | V-33 |
| **Название** | Release Pipeline Never Run — Artifacts Non-Reproducible |
| **STRIDE** | Tampering |
| **Описание** | `.github/workflows/release.yml` содержал дефекты (build-job не объявлял `outputs.hashes`, 5 таргетов вместо 6) и ни разу не запускался (нет тегов `v*` в GitHub Actions). Артефакты reproducible builds (cosign, SLSA provenance) существовали только на бумаге. |
| **Mitigation** | Workflow исправлен в D03 (commit `60e1840`): добавлен job `aggregate-hashes` для SLSA subject, 6-й таргет `aarch64-pc-windows-msvc`. Pre-flight фиксы BUG-S1-001 (commit `a830c51`, 2026-10-10): коллизия `hash.txt` в aggregate (merge-multiple removed), `base64-subjects` по контракту SLSA-генератора, `RUSTFLAGS` expression expansion (неэкспандируемый `$GITHUB_WORKSPACE` был silent no-op), ретированный `macos-13` → `macos-latest`, публикация `SHA256SUMS.txt`. |
| **Stage** | D03 (workflow fix) → **closed BUG-S1-001 (2026-10-10)** |
| **Residual Risk** | **Низкий.** Закрыто первым запуском: тег `v0.0.1-rc1` → run https://github.com/Reider85/strangecoin/actions/runs/38071514479 green (6/6 builds + aggregate + sign + SLSA + release, 31 asset). Независимая верификация: SHA256-сверка windows/linux x86_64 = MATCH; `cosign verify-blob` = `Verified OK`; `slsa-verifier` = `PASSED` @ commit `a830c51`. Evidence: `docs/security/REPRODUCIBLE_BUILDS.md` §Verified Release Runs. |
| **Monitoring** | Мониторинг GitHub Actions после каждого `v*`-тега; alert при failures/pipeline anomalies; сверять `SHA256SUMS.txt` при выпуске. |

---

## 5. Additional Stage 1-Specific Vectors

Добавлено в S1-P21 (DoD Stage 1: «threat model актуализирован, если Stage добавляет новые attack vectors»). Нумерация продолжает V-33. Каждый вектор: Mitigation → промпт S1-PXX → код → тест; Residual Risk и Monitoring заполнены.

---

#### V-34: Headers-First Poisoning

| Поле | Значение |
|------|----------|
| **ID** | V-34 |
| **Название** | Headers-First Poisoning |
| **STRIDE** | Tampering / Denial of Service |
| **Описание** | Заголовки принимаются до тел блоков. Атакующий шлёт поток фальшивых заголовков (с валидным PoW-«островом» или без) для: (1) захвата выбора вершины через подделанный cumulative work, (2) исчерпания ресурсов узла валидацией мусорных заголовков, (3) отравления header-cache. |
| **Mitigation** | PoW-проверка каждого заголовка (`validate_header_pow`: declared-hash + PoW) до включения в cache; проверка parent linkage и index continuity — `HeaderCache` останавливается на первом невалидном заголовке (bad header никогда не доходит до tip selection); выбор вершины по cumulative work через chain_selector; batch-лимиты `MAX_HEADERS_BATCH=2000`/`MAX_BLOCKS_BATCH=128` + size-checks до аллокации (инварианты #7/#16); rate limiter + ban по пире; полный `validate_chain` для тел блоков не ослаблен (заголовки только прокладывают маршрут). |
| **Stage** | S1-P16 |
| **Residual Risk** | Низкий/средний. PoW-проверка заголовков стоит CPU — при very-low difficulty (regtest) flood возможен, но rate limiter + ban ограничивают. Noise Protocol (Stage 2) усилит аутентификацию пиров. Cumulative work считается на заголовках — при компромете майнера >50% вектор деградирует в 51% attack (V-05). |
| **Monitoring** | Мониторинг rejected headers rate per peer; alert при всплеске; chain tip divergence > 1 блока (partition indicator). |
| **Код** | `src/network/sync.rs` (HeaderCache, plan_best_branch, sync_headers_first), `src/network/protocol.rs` (4 сообщения), `crates/strangecoin-core/src/consensus.rs` (validate_header_pow, cumulative_work_headers) |
| **Тест** | `tests/sync_headers.rs::new_node_syncs_20_blocks_via_headers_first`, `::equal_chain_reports_nothing_better`, `::longer_fork_resolved_via_headers_first` |

---

#### V-35: State Root Manipulation

| Поле | Значение |
|------|----------|
| **ID** | V-35 |
| **Название** | State Root Manipulation |
| **STRIDE** | Tampering |
| **Описание** | `block.state_root` (SMT, ADR-0006 amended) коммитит post-state. Атакующий: (1) подделывает state_root в блоке (взломанный или невалидный корень), (2) пытается провести блок с корнем, не соответствующим применённому state, (3) ~~эксплуатирует опциональный «zero root = no commitment» для обхода проверок~~ — **закрыто BUG-S1-002 (SCIP-0002, 2026-10-11)**. |
| **Mitigation** | Инвариант №19 enforce: `root_after(parent_state, block, allow_zero_state_root) == block.state_root`, иначе typed error `StateRootMismatch` → reject; apply_block детерминирован (канонический порядок tx); zero state_root отвергается на mainnet/testnet **без opt-out** (config-gate: `allow_zero_state_root=true` вне regtest — config error); genesis (index 0) освобождён; legacy regtest opt-in — network-aware default `Config.allow_zero_state_root` (SCIP-0002); mine/grant-пути коммитят реальный корень. |
| **Stage** | S1-P06 (ADR-0006), enforced в S1-P12 block_executor; zero-root opt-out closed BUG-S1-002 (SCIP-0002) |
| **Residual Risk** | Низкий. Mainnet/testnet: commitment обязателен, обхода нет. Regtest opt-in — только developer-среда без реальной ценности. Stateless post-root-проверка (light/SPV) **закрыта BUG-S1-003 (2026-10-11)** — `verify_block_stateless` пересчитывает и сверяет post-root (V-36). |
| **Monitoring** | Мониторинг StateRootMismatch reject rate; alert при всплеске (potential attack или desync). |
| **Код** | `crates/strangecoin-core/src/state/sparse_merkle.rs`, `state/mod.rs` (root_after), `src/blockchain/block_executor.rs`, `src/config.rs` (allow_zero_state_root gate) |
| **Тест** | `tests/state_root.rs::tampered_state_root_is_rejected_by_the_second_node`, `::committed_state_root_chain_passes_on_a_second_node`, `::zero_state_root_chain_rejected_without_the_opt_in_flag`, `::grant_and_mined_blocks_commit_real_state_roots`; core `tests/state_root.rs::tamper_state_root_rejected`, `::zero_state_root_rejected_without_opt_in`, `::genesis_zero_state_root_is_always_tolerated`, `::proptest_root_consistent`; `tests/block_executor.rs::rejects_state_root_that_does_not_match_the_applied_state`, `::rejects_zero_state_root_when_commitment_required`, `::accepts_zero_state_root_with_opt_in_flag`; `src/config.rs` unit-тесты network-aware default + mainnet reject |

---

#### V-36: Witness Spoofing

| Поле | Значение |
|------|----------|
| **ID** | V-36 |
| **Название** | Witness Spoofing |
| **STRIDE** | Tampering / Spoofing |
| **Описание** | StateWitness (пробы SMT для затронутых адресов) позволяет верифицировать блок без полного state. Атакующий: (1) подделывает значения балансов/nonce в witness, (2) подставляет witness с неверными адресами, (3) манипулирует parent root в stateless-проверке, (4) ~~подставляет произвольный `state_root` в заголовке при корректных pre-proofs~~ — **закрыто BUG-S1-003 (2026-10-11)**, (5) ~~коммитит post-root с тихо изменённым untouched-аккаунтом~~ — **закрыто BUG-S1-003**. |
| **Mitigation** | `verify_block_stateless(parent_state_root, block, witness)` пересчитывает post-state-root из parent root + witness + блока: pre-proofs проверяются против parent root, block применяется к partial state, затем `SparseMerkleTrie::root_after_updates` (mulproof update: seed frontier из proofs → overwrite leaves → bottom-up recompute) re-derive post-root и сравнивает с `block.state_root` → typed `PostStateRootMismatch` → reject; untouched-листы сохраняют pre-хеши, привязанные к parent root — разойтись без поломки pre-proof нельзя. Witness обязан покрывать все touched-адреса (`Err(WitnessVerificationFailed)`). Genesis-exempt (`index == 0 && state_root == [0;32]`) зеркалит `root_after`/SCIP-0002. Tamper-тесты на уровне ядра; minimality: witness не содержит лишних адресов (регрессионный критерий). Full-node валидация продолжает работать по полному state (не ослаблена). |
| **Stage** | S1-P07 (+ закрытие residual BUG-S1-003, 2026-10-11) |
| **Residual Risk** | Низкий. Пересчёт полный (произвольный state_root и untouched-дивергенция закрыты). Остаточный риск Stage 2+: witness API ещё не в сетевом протоколе — спуфинг материализуется только когда light-клиенты начнут принимать witness по сети (Noise + аутентификация full-node). Prover-сторона полагается на полный pre-state — компромет full-node может выдать невалидный witness, но light-client его отклонит по post-root mismatch. |
| **Monitoring** | N/A на Stage 1 (нет wire-протокола witness). Мониторинг — при введении witness-сообщений в Stage 2+. |
| **Код** | `crates/strangecoin-core/src/state/witness.rs` (`verify_block_stateless`), `state/sparse_merkle.rs` (`root_after_updates`) |
| **Тест** | `crates/strangecoin-core/tests/witness.rs::tampered_balance_rejected`, `::proptest_tamper_detected`, `::wrong_parent_root_rejected`, `::witness_contains_only_touched_addresses`, `::forged_state_root_rejected`, `::untouched_account_tamper_in_post_commitment_rejected`, `::zero_state_root_on_non_genesis_rejected`, `::missing_touched_address_rejected`, `::genesis_zero_state_root_is_tolerated`, `::proptest_forged_state_root_rejected`; core `sparse_merkle.rs` unit `root_after_updates_*` + proptest |

---

#### V-37: Tx Root Manipulation

| Поле | Значение |
|------|----------|
| **ID** | V-37 |
| **Название** | Tx Root Manipulation |
| **STRIDE** | Tampering |
| **Описание** | `block.tx_root` (merkle root транзакций) — SPV-коммитмент. Атакующий: (1) подменяет список tx в блоке, оставив неверный tx_root, (2) подделывает tx_root под чужой набор tx, (3) манипулирует порядком tx для divergent root. |
| **Mitigation** | Enforce: `merkle_root(txids блока) == block.tx_root`, иначе `TxRootMismatch` → reject; merkle детерминирован (blake3-пары, дублирование последнего при нечётности, канонический порядок tx — тот же, что в сериализации); tamper-тесты; property-тесты (чёт/нечёт, перестановка → другой root, но оба валидны при пересчёте). |
| **Stage** | S1-P08 |
| **Residual Risk** | Низкий. Коммитмент обязателен в заголовке (format_version bump, mainnet не запущен — миграция бесплатна). Residual связан с SPV-клиентами Stage 2+ (не проверяют PoW достаточно глубоко). |
| **Monitoring** | Мониторинг TxRootMismatch reject rate; alert при аномалиях. |
| **Код** | `crates/strangecoin-core/src/serialize.rs` (merkle_root, compute_tx_root), `src/blockchain/block_executor.rs` (проверка) |
| **Тест** | `crates/strangecoin-core/tests/merkle.rs::empty_txids_returns_zero`, `::single_txid_duplicated`, `::two_txids_pair_hash`, `::three_txids_odd_duplication`, `::seven_txids_multi_level`, `::proptest_deterministic_root`; `tests/block_executor.rs::rejects_wrong_tx_root` |

---

#### V-38: Consensus Version Downgrade

| Поле | Значение |
|------|----------|
| **ID** | V-38 |
| **Название** | Consensus Version Downgrade |
| **STRIDE** | Tampering / Elevation of Privilege |
| **Описание** | `block.consensus_version` определяет набор правил. Атакующий: (1) шлёт блоки с устаревшей версией, чтобы обойти новые правила (downgrade attack), (2) шлёт будущую версию, чтобы обрушить валидацию узлов со старым кодом, (3) манипулирует activation height через SCIP. |
| **Mitigation** | `validate_chain` проверяет `block.consensus_version` против `current_consensus_rules(height)` — версия ниже активной → reject; SCIP skeleton (governance/scip.rs): activation height, backward compatibility (старые узлы принимают блоки до activation, после — отвергают); единый источник `CURRENT_CONSENSUS_VERSION`; dummy-правило с enforce-тестом по высоте. |
| **Stage** | S1-P05 |
| **Residual Risk** | Низкий. Механизм работает; контентных SCIP пока нет (mainnet не запущен). Residual: при будущих активациях — социальная координация апгрейда (hard fork window). TLA+ spec не моделирует version switching (см. §7). |
| **Monitoring** | Мониторинг consensus_version reject rate; alert при reject на testnet/mainnet (возможен downgrade-попытка или misconfigured peer). |
| **Код** | `crates/strangecoin-core/src/governance/scip.rs`, `consensus.rs` (CURRENT_CONSENSUS_VERSION), `src/blockchain/consensus_manager.rs` |
| **Тест** | `tests/consensus_version.rs::stale_consensus_version_rejected`, `::future_consensus_version_rejected`, `::correct_consensus_version_accepted`; `tests/block_executor.rs::rejects_stale_consensus_version` |

---

#### V-39: Network Downgrade / Confusion

| Поле | Значение |
|------|----------|
| **ID** | V-39 |
| **Название** | Network Downgrade / Confusion |
| **STRIDE** | Spoofing |
| **Описание** | Узел regtest теоретически может соединиться с узлом другой сети и получить «валидный» для себя мусор. Атакующий: (1) подставляет HELLO с чужим network_id, (2) мешает handshake для downgrade на более слабую сеть, (3) путает chain_id транзакций с чужой сетью (replay). |
| **Mitigation** | `network_id` в genesis.json и в HELLO-сообщении; при handshake сверяет со своей сетью: mismatch → disconnect + ban + warn-лог, ДО обработки любых других данных; chain_id == network_id из единого источника констант (core::consensus: mainnet=1, testnet=2, regtest=3); EXPECTED_GENESIS_HASH обновлён per network. |
| **Stage** | S1-P14 (+ константы из S1-P03/S1-P05) |
| **Residual Risk** | Низкий. Изоляция протокольная и на уровне генезиса. Residual: plaintext TCP (Stage 2 Noise) — MITM может подменить DNS/список seeds (V-16), но не network_id валидного HELLO без компромета endpoints. |
| **Monitoring** | Мониторинг HELLO network_id mismatch rate; alert при всплеске (recon или misconfiguration). |
| **Код** | `genesis.json` (network_id), `src/network/protocol.rs` (HELLO + проверка), `crates/strangecoin-core/src/consensus.rs` (CHAIN_ID-константы) |
| **Тест** | `tests/network_id.rs::foreign_network_id_is_rejected_and_banned`; связность chain_id/network_id — proptest `crates/strangecoin-core/tests/consensus_proptest.rs::chain_id_validation` |

---

#### V-40: RBF Fee-War DoS

| Поле | Значение |
|------|----------|
| **ID** | V-40 |
| **Название** | RBF Fee-War DoS |
| **STRIDE** | Denial of Service |
| **Описание** | Mempool RBF (replace-by-fee): атакующий шлёт бесконечные замены tx (один sender + nonce), чтобы: (1) сжигать CPU на re-validation, (2) вытеснять чужие tx из mempool, (3) провоцировать fee-war, (4) через RBF попытаться открыть double-spend (замена не-найденной tx). |
| **Mitigation** | Детерминированные правила: feerate = fee/weight (пока fee=0 → proxy 1/serialized_len); замена только если new feerate ≥ old × (1 + `RBF_MIN_DELTA`); `MAX_RBF_REPLACEMENTS` (анти-DoS лимит цепочки замен); все insert-проверки обязательны при замене; nonce-правило приоритетнее RBF (RBF не открывает двойную трату); анонс `TxRejected { reason: Replaced }` через EventBus. |
| **Stage** | S1-P17 |
| **Residual Risk** | Низкий. Лимиты + feerate-delta ограничивают fee-war; real fee market (EIP-1559) — Stage 5. При fee=0 feerate — прокси по размеру: спам одинаковых tx возможен, но ограничен MAX_PENDING_TXS + rate limiter. |
| **Monitoring** | Мониторинг RBF replacement rate per sender; alert при аномальном количестве замен; mempool size > 80%. |
| **Код** | `src/mempool/mod.rs` (feerate, find_replaceable, RBF_MIN_DELTA, MAX_RBF_REPLACEMENTS) |
| **Тест** | `tests/rbf.rs::rbf_replacement_emits_tx_rejected`, `::rbf_replacement_with_lower_feerate_rejected`, `::rbf_replacement_chain_is_limited`, `::rbf_replacement_evicts_dependencies`, `::rbf_replacement_is_what_gets_mined`; `tests/double_spend.rs` green (регрессий нет) |

---

#### V-41: SyncEngine Inbox Flooding

| Поле | Значение |
|------|----------|
| **ID** | V-41 |
| **Название** | SyncEngine Inbox Flooding |
| **STRIDE** | Denial of Service |
| **Описание** | SyncEngine — единственный validate→apply→announce; входящие блоки/заголовки/tx идут через inbox (mpsc). Атакующий: (1) переполняет inbox дубликатами/мусором, (2) флудит BLOCKS, чтобы HEADERS легитимных пиров задерживались, (3) давит на single-consumer порядок для лагов. |
| **Mitigation** | Bounded inbox (backpressure); single-consumer строго последовательная обработка (детерминизм); приоритет: дубликаты дропаются, спамеры банятся через rate limiter, HEADERS обрабатываются до BLOCKS того же пира; network-обработчики только кладут в inbox (никаких прямых вызовов blockchain — rg-аудит чистый); lane separation (full pull lane / inbound lane) с ban на переполнение. |
| **Stage** | S1-P18 (ADR-0010) |
| **Residual Risk** | Низкий/средний. Bounded + ban ограничивают флуд; при недобросовестных пирах с высоким rate legit-блоки могут задерживаться до rate-limit окна. Gossip-оптимизации (Erlay) — Stage 2. |
| **Monitoring** | Мониторинг inbox full / dropped events; alert при частых переполнениях; ban rate per peer. |
| **Код** | `src/network/sync_engine.rs`, `src/network/mod.rs` (только inbox), `src/network/sync.rs` |
| **Тест** | `tests/sync_engine.rs::concurrent_candidates_race_through_one_engine`; unit-тесты `src/network/sync_engine.rs::duplicate_candidate_is_dropped_at_the_inbox`, `::full_pull_lane_drops_without_ban`, `::full_inbound_lane_bans_the_producer`, `::headers_lane_is_separate_from_blocks`; `tests/network.rs::real_network_fast_registration_race` |

---

#### V-42: HRP Confusion

| Поле | Значение |
|------|----------|
| **ID** | V-42 |
| **Название** | HRP Confusion (Cross-Network Address) |
| **STRIDE** | Spoofing |
| **Описание** | Перевод средств между сетями по человеческому фактору: пользователь копирует адрес regtest (rsc1...) и отправляет mainnet-средства, или вставляет битый адрес без контрольной суммы (Stage 0: base64-pubkey без checksum — ARCHITECT2 §1.1 №6). Спуфинг: подделка адреса в UI/клипборде без визуального отличия сетей. |
| **Mitigation** | bech32 с HRP, несущим network: `sc1`/`tsc1`/`rsc1` (mainnet/testnet/regtest); checksum ошибки → typed error; чужой HRP → Err; все точки создания адреса (wallet, GUI, CLI, genesis) переведены на encode_address(pubkey, network_id); base64-pubkey-адресов выведены из кода; миграция legacy-DB — пересчёт адреса из pubkey. |
| **Stage** | S1-P15 |
| **Residual Risk** | Низкий. Checksum + HRP закрывают случайные ошибки. Residual: пользователь может намеренно/ошибочно ввести валидный HRP другой сети — UI должен визуально отличать префиксы (требование к GUI Stage 2+). |
| **Monitoring** | Мониторинг reject из-за HRP/checksum mismatch; alert при систематических ошибках (potential UX issue или адрес-спуфинг). |
| **Код** | `crates/strangecoin-core/src/address.rs` (encode_address/decode_address, bech32::Hrp), `src/address.rs` (re-export) |
| **Тест** | `crates/strangecoin-core/src/address.rs::test_round_trip`, `::test_checksum_error`, `::test_hrp_validation`, `::test_network_id_hrp_mapping`; интеграционный bech32-transfer на `rsc1` — `tests/two_clients.rs::bech32_address_transfer`; миграция legacy-DB (base64→bech32) — `tests/address_migration.rs::legacy_base64_balances_migrate_to_bech32_on_open` |

---

## 6. Mitigations Map

### 6.1 Prompt → Vector Mapping (Stage 0)

| Prompt | Векторы, которые он mitigates |
|--------|------------------------------|
| **P01** | V-16 (DNS poisoning — seeds configuration) |
| **P04** | V-23 (key compromise — keystore encryption), V-27 (keystore theft) |
| **P05** | V-11 (cross-chain replay — chain_id), V-12 (intra-chain replay — nonce), V-26 (testnet isolation), V-30 (nonce manipulation) |
| **P06** | V-22 (compromised build — canonical serialization), V-03 (selfish mining — block hash) |
| **P07** | V-12 (replay — txid commitment), V-05 (51% — signature verification) |
| **P08** | V-04 (time-warp — difficulty retargeting), V-05 (51% — difficulty validation) |
| **P09** | V-04 (time-warp — MTP + future timestamp) |
| **P10** | V-29 (genesis manipulation — EXPECTED_GENESIS_HASH) |
| **P11** | V-05 (51% — emission validation, partial) |
| **P12** | V-13 (OOM — size limits), V-15 (invalid blocks — size validation) |
| **P13** | V-01 (eclipse — rate limiting), V-14 (spam txs — rate limit), V-15 (invalid blocks — ban), V-17 (sybil — connection limits) |
| **P14** | V-14 (spam txs — mempool cap), V-30 (nonce — mempool validation) |
| **P15** | V-28 (config leakage — secrets separation) |
| **P22 → D02** | All vectors (documentation, monitoring) |
| **P24 → D03** | V-22 (compromised build — reproducible builds, cosign, SLSA); V-33 (workflow fix) |

### 6.2 Prompt → Vector Mapping (Stage 1, добавлено S1-P21)

| Prompt | Векторы, которые он mitigates |
|--------|------------------------------|
| **S1-P01** | V-32 (grant blocks → regtest-only flag; закрыт) |
| **S1-P05** | V-38 (consensus_version + activation height), V-39 (константы chain_id/network_id) |
| **S1-P06** | V-35 (state_root — инвариант #19) |
| **S1-P07** | V-36 (witness spoofing — tamper tests) |
| **S1-P08** | V-37 (tx_root — merkle enforce) |
| **S1-P14** | V-39 (network_id в genesis + HELLO) |
| **S1-P15** | V-42 (bech32 HRP) |
| **S1-P16** | V-34 (headers-first poisoning) |
| **S1-P17** | V-40 (RBF fee-war DoS) |
| **S1-P18** | V-41 (SyncEngine inbox flooding) |
| **S1-P19** | Fuzz/soak canonical decoders (V-13 adjacent; Monitoring §8.4) |
| **S1-P20** | Ре-аудит 22 инвариантов (all vectors — no regressions) |
| **S1-P21** | V-34..V-42 (документирование этого раздела) |

### 6.2b Prompt → Vector Mapping (Stage 1.5, добавлено S1.5-P18)

| Prompt | Векторы, которые он mitigates |
|--------|------------------------------|
| **S1.5-P06** | V-21 (wasmi sandbox, crate + WasmiExecutor) |
| **S1.5-P09** | V-21 (gas metering — infinite loops), V-20 (block gas limit как глобальный ceiling) |
| **S1.5-P10** | V-21 (host functions whitelist — 13 функций) |
| **S1.5-P11** | V-19 (modexp precompile + checked dispatcher lengths) |
| **S1.5-P13** | V-18 (reentrancy guard: non_reentrant + call_stack), V-19 (MAX_CODE_SIZE в verify_transaction), V-20 (MAX_STORAGE_PER_CONTRACT), V-21 (MAX_MEMORY_PAGES / MAX_STACK_HEIGHT / MAX_CALL_DEPTH) |
| **S1.5-P14** | Инвариант #18 (Receipt на каждый tx — audit trail для VM-атак) |
| **S1.5-P15** | Инвариант #15 (enforce_receipt_gas_limit post-execution), инвариант #19 extension (contract storage в state root), V-20 (flush только в пределах cap) |
| **S1.5-P16** | V-21 (differential wasmi vs wasmtime), V-19/V-20 adjacent (fuzz gas_metering / storage_trie / host_functions) |
| **S1.5-P17** | V-18 (e2e reentrancy attack rejected), V-20/V-21 (e2e gas exhaustion) |
| **S1.5-P18** | V-18..V-21 (документирование митигаций этого раздела; regression 22 инвариантов) |

### 6.2c Prompt → Vector Mapping (Stage 2, добавлено S2-P23)

| Prompt | Векторы, которые он mitigates |
|--------|------------------------------|
| **S2-P07** | V-24 (Noise XX handshake state machine, forward secrecy) |
| **S2-P08** | V-24 (ChaCha20-Poly1305 transport, anti-replay nonces), V-13 adjacent (framing size check до аллокации — инвариант #16 ext) |
| **S2-P09** | V-24 (Ed25519 node_id auth — MITM detection), V-17 adjacent (Sybil требует per-node keypair) |
| **S2-P10** | V-39 regression (HELLO network_id, MIN_PROTO_VERSION=2), V-01 adjacent (capabilities gating) |
| **S2-P11** | V-01 (multi-source bootstrap), V-16 (DNSSEC validation + hard-coded IPs + ADDR gossip) |
| **S2-P12** | V-17 (per-IP-range connection cap), V-01 (peer store/manager + ban state machine) |
| **S2-P13** | V-14 adjacent (INV dedup ограничивает gossip-flood), V-41 adjacent (per-peer dedup) |
| **S2-P14** | V-13 adjacent (compact-block size caps), инвариант #7 regression |
| **S2-P15** | Erlay reconciliation (bandwidth scaling; fallback full relay — no tx loss) |
| **S2-P16** | V-14 (token bucket + per-message caps + ban threshold, ADR-0023) |
| **S2-P17** | V-15 (invalid_blocks factor + ban ladder), V-17 (scoring + bans), V-01 (scoring), ADR-0022 |
| **S2-P18** | V-13 adjacent (deprecated plaintext-сообщения удалены — меньше attack surface) |
| **S2-P19** | V-24 (единый Noise-транспорт, plaintext-путь удалён), V-02 (sync через encrypted сессии) |
| **S2-P21** | Fuzz 4 network targets: noise_handshake / gossip_inv / compact_blocks_fill_up / erlay_reconciliation (V-24/V-13/V-14 adjacent, CI-side) |
| **S2-P22** | Интеграционные тесты: V-01/V-14/V-15/V-17/V-24 (`tests/net_*.rs`) |
| **S2-P23** | V-1/V-2/V-14/V-15/V-16/V-17/V-24 (документирование митигаций этого раздела; regression 22 инвариантов — `docs/stage2/INVARIANTS_ENFORCED.md`) |

### 6.3 Coverage Matrix

| STRIDE Category | Covered by Stage 0 | Covered by Stage 1 | Covered by Stage 1.5 | Covered by Stage 2 | Residual (deferred) |
|-----------------|--------------------|--------------------|----------------------|--------------------|---------------------|
| **Spoofing** | P05 (chain_id), P13 (peer limits) | S1-P14 (network_id HELLO), S1-P15 (bech32 HRP) | — | V-24 (Noise XX + Ed25519 node_id auth), V-01 (multi-source bootstrap + scoring), V-16 (DNSSEC), V-17 (per-IP cap + scoring) | V-06 (long-range → Stage 7) |
| **Tampering** | P06 (serialization), P07 (txid), P08-P09 (difficulty/time) | S1-P06 (state_root), S1-P08 (tx_root), S1-P05 (consensus_version), S1-P16 (headers) | V-18 (reentrancy guard), V-19 (checked arith + modexp + MAX_CODE_SIZE), V-21 (wasmi sandbox + host whitelist + gas) | V-24 (AEAD transport — сообщения не модифицируемы без детекта) | V-07 (nothing-at-stake → Stage 7) |
| **Repudiation** | P07 (txid commitment) | — | Receipts (#18): audit trail каждого VM-call | — | — |
| **Information Disclosure** | P04 (keystore), P15 (config) | S1-P07 (witness minimality) | — | V-24 (P2P трафик зашифрован; MEV sniffing на relay-слойе закрыт) | V-08, V-10 (MEV on-chain → Stage 5) |
| **Denial of Service** | P12 (size limits), P13 (rate limiting), P14 (mempool) | S1-P17 (RBF limits), S1-P18 (inbox bounded) | V-20 (storage cap 1MB + block gas limit 1M), V-21 (memory/stack/depth caps) | V-14 (token bucket + scoring), V-15 (scoring ban ladder), V-17 (conn cap + scoring), V-41 (gossip dedup) | V-09 (censoring → Stage 7); global state growth без rent → Stage 3+ |
| **Elevation of Privilege** | P05 (nonce), P10 (genesis) | S1-P01 (grant flag), S1-P05 (version enforce) | V-21 (sandbox: no host I/O вне whitelist) | — | V-05 (51% → monitoring), V-23 (key compromise → Stage 5 AA) |

### 6.4 Mitigations → Test Coverage Map

Каждый вектор привязан к конкретному тесту. Если тест отсутствует — помечен как **gap**.

| Вектор | Модуль/Функция | Тест | Статус |
|--------|---------------|------|--------|
| V-01 (Eclipse) | discovery (multi-source) + peer scoring + pool per-IP cap + rate_limiter | `tests/net_peer_scoring.rs` (3), `tests/net_helpers.rs` (bootstrap); regression: `tests/network.rs::real_network_three_nodes`, `tests/rate_limit.rs` | ✅ D01 + BUG-S1-005 + **S2-P11/P12/P17** |
| V-02 (Partition) | sync_engine ≥3 peers + Noise transport | `tests/sync_engine.rs`, `tests/sync_headers.rs` (regression); `tests/net_noise_handshake.rs` (encrypted sessions) | ✅ D01 + **S2-P19** |
| V-03 (Selfish mining) | consensus/monitoring | gap: нет теста на orphan rate | ⚠️ gap |
| V-04 (Time-warp) | consensus (MTP, retarget) | `tests/time.rs::mtp_rejection`, `tests/time.rs::future_timestamp_rejection`; `tests/block_executor.rs::rejects_timestamp_*` | ✅ D01 + S1-P12 |
| V-05 (51% attack) | consensus (difficulty) | `tests/pow.rs::mining_and_validation`; `tests/block_executor.rs::rejects_proof_of_work_above_target` | ✅ D01 + S1-P12 |
| V-06 (Long-range) | N/A (PoS, Stage 7) | N/A | Deferred |
| V-07 (Nothing-at-stake) | N/A (PoS, Stage 7) | N/A | Deferred |
| V-08 (MEV frontrunning) | N/A (Stage 5) | N/A | Deferred |
| V-09 (MEV censoring) | N/A (Stage 7) | N/A | Deferred |
| V-10 (MEV sandwich) | N/A (Stage 5) | N/A | Deferred |
| V-11 (Cross-chain replay) | consensus (chain_id) + network (HELLO) | `tests/network_id.rs::foreign_network_id_is_rejected_and_banned`; proptest `chain_id_validation` | ✅ S1-P14/S1-P19 |
| V-12 (Intra-chain replay) | mempool (nonce) | `tests/double_spend.rs::double_spend_rejected` | ✅ D01 |
| V-13 (OOM) | network (framing) + serialize (bounds-checked readers) | unit-тесты `src/network/`, `src/mempool/`; soak S1-P19 (fuzz) | ✅ Stage 0 + S1-P19 |
| V-14 (Spam txs) | rate_limiter (token bucket, S2-P16) + peer scoring + mempool (cap) | `tests/net_rate_limit_integration.rs` (2), `rate_limiter.rs` unit; regression: `tests/two_clients.rs` | ✅ D01 + **S2-P16/S2-P17** |
| V-15 (Invalid blocks) | peer scoring (invalid_blocks factor + ban ladder) + HeaderCache + block_executor | `tests/net_peer_scoring.rs::malicious_peer_banned_after_configured_invalid_block_threshold`, `::persistent_ban_survives_restart_via_network_json`; regression: `tests/block_executor.rs`, `tests/sync_headers.rs` | ✅ **S2-P17** (gap ban-after-N закрыт) |
| V-16 (DNS poisoning) | discovery (DNSSEC + hard-coded IPs + ADDR gossip) | `tests/net_helpers.rs` (multi-source bootstrap); discovery unit (dnssec policy) | ✅ **S2-P11** (gap закрыт) |
| V-17 (Sybil) | pool per-IP-range cap + peer scoring + rate_limiter | `tests/net_peer_scoring.rs` (3), `tests/net_rate_limit_integration.rs` (2), pool unit (per-IP reject); regression: `tests/rate_limit.rs` | ✅ **S2-P12/S2-P16/S2-P17** (gap connection limits закрыт) |
| V-18 (Reentrancy) | vm-wasm (call_stack + `non_reentrant` ABI) + core limits | `tests/vm_reentrancy_attack.rs::reentrancy_attack_end_to_end_is_rejected`; vm-wasm `tests/reentrancy_and_limits.rs::self_call_with_non_reentrant_abi_is_rejected`, `::cross_contract_reentrancy_is_rejected` | ✅ S1.5-P13/P17 |
| V-19 (Integer overflow) | core limits (MAX_CODE_SIZE) + precompiles (modexp) + Rust checked arith | `tests/reentrancy_and_limits.rs::create_code_size_over_limit_rejected`; `tests/precompiles.rs::modexp_*` (10) | ✅ S1.5-P11/P13 |
| V-20 (Storage exhaustion) | vm-wasm storage_write (1MB cap) + block gas limit | `tests/reentrancy_and_limits.rs::storage_cap_at_limit_accepted`, `::storage_cap_exceeded_rejected`; `tests/vm_gas_exhaustion.rs`; `tests/contract_storage.rs` (>1MB → Failed) | ✅ S1.5-P13/P15 |
| V-21 (Code injection) | wasmi sandbox + host whitelist + gas metering + resource limits | `tests/reentrancy_and_limits.rs::memory_*`, `::runaway_recursion_hits_stack_limit`; `tests/events_and_receipts.rs::register_code_rejects_invalid_wasm`; `tests/gas_metering.rs`; fuzz `differential_wasmi_vs_wasmtime.rs` | ✅ S1.5-P06/P09/P10/P16 |
| V-22 (Compromised build) | CI/CD (P24) | gap: GitHub Actions run не верифицирован | ⚠️ gap (→ first v* tag) |
| V-23 (Key compromise) | wallet (keystore) | unit-тесты wallet | ✅ Stage 0 |
| V-24 (MITM) | noise (XX handshake + ChaCha20-Poly1305 + Ed25519 node_id auth) | `tests/net_noise_handshake.rs` (4: roundtrip, MITM NodeId mismatch drop, unpinned TOFU); crate unit-тесты noise/*; fuzz `noise_handshake.rs` (CI-side) | ✅ **S2-P07/S2-P08/S2-P09** (deferred → mitigated) |
| V-25 (Validator collusion) | N/A (Stage 7) | N/A | Deferred |
| V-26 (Testnet low-diff) | consensus (chain_id) | `tests/pow.rs::mining_and_validation` | ✅ D01 |
| V-27 (Keystore theft) | wallet (AES-GCM) | unit-тесты wallet | ✅ Stage 0 |
| V-28 (Config leakage) | config (P15) | gap: нет теста на secrets в config | ⚠️ gap |
| V-29 (Genesis manipulation) | consensus (hash) | `tests/common/mod.rs::genesis_validation`; panic-guard при mismatch | ✅ D01 |
| V-30 (Nonce manipulation) | mempool (nonce) | `tests/double_spend.rs` | ✅ D01 |
| V-31 (Genesis key) | consensus (load_genesis pubkey-only) + SCIP-0001 | `tests/genesis_key.rs` (hash без секрета в коде, pubkey-only genesis.json, seed regression-guard); offline key — pre-mainnet ops (SCIP-0001); keygen tooling: `examples/generate_offline_genesis_key.rs` (2026-10-14) | ✅ code (S1.5-P01); ⚠️ ops (→ mainnet freeze) |
| V-32 (Grant blocks) | consensus (validate_chain) + Config flag | `tests/grant_flag.rs` (4 теста), `tests/emission.rs`, `tests/block_executor.rs::grant_block_needs_the_opt_in_flag` | ✅ S1-P01 |
| V-33 (Release pipeline) | CI/CD (D03 fix + BUG-S1-001) | run 38071514479 (v0.0.1-rc1) green: 6 builds + SLSA L3 + cosign + SHA256SUMS; cosign verify-blob OK, slsa-verifier PASSED @ a830c51 | ✅ BUG-S1-001 (2026-10-10) |
| V-34 (Headers-first poisoning) | network/sync.rs + consensus | `tests/sync_headers.rs::new_node_syncs_20_blocks_via_headers_first`, `::equal_chain_reports_nothing_better`, `::longer_fork_resolved_via_headers_first` | ✅ S1-P16 |
| V-35 (state_root manipulation) | core state/sparse_merkle + block_executor + config gate | `tests/state_root.rs::tampered_state_root_is_rejected_by_the_second_node`, `::committed_state_root_chain_passes_on_a_second_node`, `::zero_state_root_chain_rejected_without_the_opt_in_flag`, `::grant_and_mined_blocks_commit_real_state_roots`; core `tests/state_root.rs::tamper_state_root_rejected`, `::zero_state_root_rejected_without_opt_in`, `::proptest_root_consistent`; `tests/block_executor.rs::rejects_state_root_that_does_not_match_the_applied_state`, `::rejects_zero_state_root_when_commitment_required` | ✅ S1-P06/P12/P19 + **BUG-S1-002 (SCIP-0002, 2026-10-11)** |
| V-36 (Witness spoofing) | core state/witness + sparse_merkle (root_after_updates) | core `tests/witness.rs::tampered_balance_rejected`, `::proptest_tamper_detected`, `::wrong_parent_root_rejected`, `::witness_contains_only_touched_addresses`, `::forged_state_root_rejected`, `::untouched_account_tamper_in_post_commitment_rejected`, `::zero_state_root_on_non_genesis_rejected`, `::missing_touched_address_rejected`, `::genesis_zero_state_root_is_tolerated`, `::proptest_forged_state_root_rejected`; core `sparse_merkle.rs::root_after_updates_*` (8 unit + proptest) | ✅ S1-P07 + **BUG-S1-003 (2026-10-11)** |
| V-37 (tx_root manipulation) | core serialize + block_executor | core `tests/merkle.rs::empty_txids_returns_zero`, `::two_txids_pair_hash`, `::three_txids_odd_duplication`, `::proptest_deterministic_root`; `tests/block_executor.rs::rejects_wrong_tx_root` | ✅ S1-P08/P12 |
| V-38 (consensus_version downgrade) | governance/scip + consensus_manager | `tests/consensus_version.rs::stale_consensus_version_rejected`, `::future_consensus_version_rejected`, `::correct_consensus_version_accepted`; `tests/block_executor.rs::rejects_stale_consensus_version` | ✅ S1-P05/P12 |
| V-39 (Network confusion) | protocol HELLO + genesis | `tests/network_id.rs::foreign_network_id_is_rejected_and_banned`; proptest `crates/strangecoin-core/tests/consensus_proptest.rs::chain_id_validation` | ✅ S1-P14/P19 |
| V-40 (RBF fee-war DoS) | mempool RBF | `tests/rbf.rs::rbf_replacement_emits_tx_rejected`, `::rbf_replacement_with_lower_feerate_rejected`, `::rbf_replacement_chain_is_limited`, `::rbf_replacement_evicts_dependencies`, `::rbf_replacement_is_what_gets_mined`; `tests/double_spend.rs` (регрессия) | ✅ S1-P17 |
| V-41 (SyncEngine flooding) | network/sync_engine | `tests/sync_engine.rs::concurrent_candidates_race_through_one_engine`; unit-тесты `src/network/sync_engine.rs::duplicate_candidate_is_dropped_at_the_inbox`, `::full_pull_lane_drops_without_ban`, `::full_inbound_lane_bans_the_producer`, `::headers_lane_is_separate_from_blocks`; `tests/network.rs::real_network_fast_registration_race` | ✅ S1-P18 |
| V-42 (HRP confusion) | core address (bech32) | core `src/address.rs::test_round_trip`, `::test_checksum_error`, `::test_hrp_validation`, `::test_network_id_hrp_mapping`; `tests/two_clients.rs::bech32_address_transfer`; `tests/address_migration.rs::legacy_base64_balances_migrate_to_bech32_on_open` | ✅ S1-P15 |

**Итого gaps:** 2 вектора без полных тестов (V-03, V-28). Из них:
- V-03, V-28 — требуют интеграционных/unit-тестов (не закрыты Stage 1/1.5/2)
- ~~V-15~~ — **closed (S2-P17, 2026-10-28)**: ban-after-N через peer scoring + `tests/net_peer_scoring.rs`
- ~~V-16~~ — **closed (S2-P11, 2026-10-26)**: DNSSEC validation + multi-source bootstrap
- ~~V-17~~ — **closed (S2-P12/S2-P16/S2-P17, 2026-10-28)**: per-IP-range connection cap + scoring + rate-limit bans
- ~~V-24~~ — **closed (S2-P07/S2-P08/S2-P09/S2-P19, 2026-10-26..28)**: Noise XX + AEAD transport + Ed25519 auth; `tests/net_noise_handshake.rs`
- ~~V-31~~ — **closed code-side** (S1.5-P01): тест genesis без секрета в коде; residual ops (offline key) — SCIP-0001 pre-mainnet
- ~~V-22/V-33~~ — **closed** (BUG-S1-001, 2026-10-10): первый release-run на GitHub Actions верифицирован (run 38071514479)
- ~~V-35 zero-root вектор (3)~~ — **closed** (BUG-S1-002, 2026-10-11): zero-root opt-out удалён (SCIP-0002); mainnet/testnet обязателен commitment
- **V-18..V-21 — closed (S1.5-P13/P17, 2026-10-17)**: reentrancy guard, storage cap, code-size cap, sandbox+limits — все с тестами (§3.7)
- Новые V-34..V-42 — все с тестами, gaps нет

---

## 7. Residual Risks

### 7.1 Risks Deferred to Later Stages

| Risk | Deferred To | Mitigation Status | Impact |
|------|-------------|-------------------|--------|
| MEV (V-08, V-09, V-10) | Stage 5 | No threshold encryption | High |
| ~~Smart Contract Attacks (V-18-V-21)~~ | ~~Stage 1.5+~~ | **Closed S1.5-P13/P17 (2026-10-17)** — см. §3.7; residual ниже в §7.1b | N/A |
| PoS-Specific Attacks (V-06, V-07, V-25) | Stage 7 | No PoS | N/A |
| ~~Network MITM (V-24)~~ | ~~Stage 2~~ | **Closed S2-P07/P08/P09 (2026-10-26..28)** — Noise XX + ChaCha20-Poly1305 + Ed25519 auth; residual: TOFU unpinned first contact + seed-derived ephemeral trade-off (§3.8 V-24) | N/A |
| Account Abstraction (V-23) | Stage 5 | No social recovery | Medium |
| Inclusion Lists (V-09) | Stage 7 | No censorship resistance | Medium |
| Light client / SPV (V-36 wire) | Stage 3+ | Witness API только в ядре, не в протоколе (stateless validation wire — Stage 3) | Medium |
| TLA+ model: Verkle/state root/reorg | Stage 1 (spec update) | `docs/spec/README.md`: свойства state root/reorg — вне scope skeleton | Low |
| ~~Gossip/Noise/Erlay~~ | ~~Stage 2~~ | **Closed S2-P13/S2-P14/S2-P15/S2-P07..P09 (2026-10-26..28)** — INV gossip, compact blocks, Erlay reconciliation, Noise transport | N/A |
| TLA+ formal verification of network protocol | Stage 6 | Stage 2 не расширяет TLA+ для network (residual зафиксирован в `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`) | Medium |

### 7.1b Stage 1.5 Residual Risks (после закрытия V-18..V-21)

| Risk | Deferred To | Mitigation Status (Stage 1.5) | Impact |
|------|-------------|-------------------------------|--------|
| Block-STM (parallel execution) | Stage 3+ (anti-goal Stage 1.5) | Serial execution; parallel re-execution может изменить interleaving gas/receipts — требует deterministic parallelISM design | Medium |
| Account Abstraction (social recovery, multisig по телу tx) | Stage 5 | Контракты могут мультиподписывать off-pattern, но protocol-level AA нет | Medium |
| EIP-1559 dynamic base fee | Stage 5 | Сейчас фиксированный block gas limit + fee=0; fee-market DoS-векторы (feespiral) вне scope | Medium |
| State rent / global state cap | Stage 3+ | Per-contract 1MB cap закрывает单体 DoS, но не спам множеством контрактов | Medium |
| Receipts fold в header (light-client receipts) | Stage 2+ | Receipts — derived data + sub-trie proofs; не в header → light clients не могут verify receipts без full node | Low |
| wasmi interpreter 0-days | continuous (fuzz S1.5-P16 + CI) | Differential testing wasmi vs wasmtime; fuzz-таргеты; formally verified VM — Stage 6+ (K-framework obligation) | Medium |
| Контракт-level бизнес-логика (rug-pull, ABI misuse) | Stage 4+ (SDK, audits) | Sandbox не защищает от «легитимно злонамеренного» кода | Medium |

### 7.1c Stage 2 Residual Risks (после закрытия V-24 / V-01 / V-14..V-17 amendments, добавлено S2-P23)

Полный перечень — `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` (S2-P23). Ключевые строки:

| Risk | Deferred To | Mitigation Status (Stage 2) | Impact |
|------|-------------|-----------------------------|--------|
| Snapshot sync | Stage 3 | Нет; новый attack surface (trusting snapshot без полной replay-верификации) | Medium |
| RocksDB migration | Stage 3 | LevelDB остаётся; crash-recovery testing — Stage 3 | Medium |
| Block-STM parallel execution | Stage 3+ | Serial execution; deterministic parallelISM design required | Medium |
| Stateless validation witnesses (wire) | Stage 3 | Witness API в ядре (S1-P07), не в протоколе; witness forgery — Stage 3 attack surface | Medium |
| Erlay minisketch full impl (PinSketch) | SCIP-gated (Stage 3+) | Documented stub (`impl_id=0`); minisketch-rs optional default-off (нет libclang на Windows-хосте) | Low |
| TLA+ формальная верификация network protocol | Stage 6 | `consensus.tla` только для консенсуса; network-протокол не формализован | Medium |
| Seed rotation (автоматическая) | Stage 6+ (или SCIP) | Ручная ротация через network.json; auto-rotation отложена (V-16 residual) | Low |
| Ephemeral из long-term seed (forward-secrecy trade-off) | Continuous (S2-P09 зафиксирован) | Компрометация seed раскрывает записанные сессии узла; reproducibility выбран осознанно | Low |
| Plaintext-клиенты Stage 1.5 несовместимы | До тега `v2.0.0-stage2` (S2-P24) | Единый Noise-транспорт (S2-P19); допустимо до mainnet freeze | Low |

### 7.2 Risks Accepted (Stage 0 + Stage 1)

| Risk | Reason | Mitigation |
|------|--------|------------|
| Selfish mining (V-03) | PoW inherent risk; no finality gadget | Monitoring + alert |
| 51% attack (V-05) | Small network; low hash rate | Monitoring + alert |
| Testnet 51% (V-26) | Low testnet difficulty by design (fast blocks); chain_id protects mainnet | **Obligation: retarget hardening via SCIP post-mainnet-freeze + Stage 6 audit; testnet residual accepted** |
| Weak password (V-27) | User responsibility | PBKDF2 ≥210k iters |
| ~~Plaintext P2P (V-24)~~ | ~~Noise Protocol deferred to Stage 2~~ **closed S2-P07/P08/P09/P19 (2026-10-26..28)**: единый Noise-транспорт; residual TOFU — §3.8 V-24 | Noise XX + AEAD + Ed25519 auth |
| Genesis key from public string (V-31) | Testnet/regtest only; secret removed from code (S1.5-P01); pubkey remains burned | **Obligation: offline key + SCIP-0001 before mainnet freeze** |
| Grant blocks (V-32) | Flag-gated (S1-P01); must stay off on mainnet | Flag default false + tests |
| Release pipeline (V-33) | ~~Workflow fixed (D03); GitHub Actions run pending~~ **closed 2026-10-10 (BUG-S1-001)**: run 38071514479 green, cosign/SLSA verified | First-run evidence: REPRODUCIBLE_BUILDS.md §Verified Release Runs |
| cargo-fuzz не запускается на dev-хосте (Windows, нет MSVC/ASan) | ASan unsupported on `x86_64-pc-windows-gnu`; no MSVC; libFuzzer needs clang/MSVC | Fallback soak runner (S1-P19) нашёл и закрыл OOB-баг; измеренный 10-min soak clean (2026-10-09). **Obligation закрыта (BUG-S0-023, 2026-10-09): job `fuzz-canonical-decode` — cargo-fuzz 600s на Linux CI, каждый push/PR**; residual — результат первого CI-прогона |
| Zero state_root tolerated (opt-in no commitment) | Явное поведение для совместимости | Узлы с enforced commitments отклоняют zero root; документировано |
| RBF при fee=0 (feerate = proxy по размеру) | Fee market — Stage 5 | RBF_MIN_DELTA + MAX_RBF_REPLACEMENTS + rate limiter |
| ~~DNS poisoning (V-16)~~ | ~~Stage 0 residual~~ **closed S2-P11 (2026-10-26)**: DNSSEC validation (hickory-resolver) + multi-source bootstrap + hard-coded IPs | DNSSEC policy reject не-Secure answers; `tests/net_helpers.rs` |

### 7.3 Risk Acceptance签字

- [x] Security team reviewed residual risks (self-review S1-P21 per ARCHITECT3 §17)
- [ ] Accepted risks documented in ADR (if needed) — genesis key obligation фиксирована в D02/STAGE0_SUMMARY
- [x] Monitoring plan expanded for Stage 1 surfaces (§8)
- [x] Stage 1+ roadmap includes mitigation for high-impact deferred risks (Noise Stage 2, fee market Stage 5, PoS Stage 7)

---

## 8. Monitoring Plan

### 8.1 What to Monitor (Stage 0 + Stage 1)

| Metric | Threshold | Alert Level | Action |
|--------|-----------|-------------|--------|
| Active peers | < 8 outgoing | Warning | Check seed nodes, network config |
| Orphan/stale block rate | > 10% | Critical | Investigate selfish mining |
| Invalid block rate per peer | > 5 in 10 min | Ban | Ban peer, investigate |
| Mempool size | > 80% capacity | Warning | Evict low-fee txs |
| Reorg depth | > 3 blocks | Critical | Investigate potential attack |
| Timestamp drift | > 1 hour from MTP | Warning | Check time sync |
| Hash rate concentration | Single miner > 33% | Critical | Alert community |
| TX time-to-inclusion | > 10 blocks | Warning | Check for censorship |
| Failed decrypt attempts | > 10 in 1 min | Warning | Potential brute-force |
| Chain tip divergence | > 1 block from peers | Warning | Check network partition |
| StateRootMismatch / TxRootMismatch rejects | > threshold / spike | Critical | Investigate state manipulation or desync |
| Consensus version rejects | Any on live network | Critical | Downgrade attempt or misconfigured peer |
| HELLO network_id mismatches | Spike in bans | Warning | Recon or misconfiguration |
| Headers rejected (PoW/parent fail) | Spike per peer | Warning | Header poisoning (V-34) |
| RBF replacements per sender | Anomalous rate | Warning | Fee-war DoS (V-40) |
| SyncEngine inbox full / drops | Recurring | Warning | Inbox flooding (V-41) |
| Soak runner (canonical decode) | Non-zero exit / panic | Critical | Decoder regression — security issue |
| ReentrancyDetected rejects | Spike per contract | Warning | Attack on contract or guard regression (V-18) |
| Storage cap rejects (per contract) | Any sustained rate | Warning | Storage exhaustion attempt (V-20) |
| OutOfGas / Failed receipt rate | Anomalous vs baseline | Warning | Malicious or broken contract (V-21); check gas weights |
| Block gas limit rejects | Spike | Warning | Gas-limit DoS or mispriced weights (invariant #15) |
| wasmi trap (non-OutOfGas, unexpected) | Any | Critical | Sandbox escape attempt or interpreter bug (V-21) |
| Noise handshake failures | Spike per peer/period | Warning | MITM attempt or protocol drift (V-24) |
| NodeId mismatch drops | Recurring | Warning | MITM or misconfigured peer keys (V-24) |
| Noise `Replay` detects | Any sustained | Critical | Replay attack or nonce desync (V-24, invariant #16 ext) |
| Rate-limit bans (token bucket) | Spike per IP range | Warning | Spam flood / Sybil (V-14/V-17, invariant #17 ext) |
| Scoring bans (warn/temp/persistent) | Any on live network | Warning | Malicious peer or scoring misconfiguration (V-15/V-17) |
| Erlay fallback rate (full relay) | Sustained high | Warning | Sketch mismatch DoS or impl drift (ADR-0021) |
| DNSSEC validation failures | Spike | Warning | DNS poisoning attempt (V-16) |
| Per-IP-range connection rejections | Spike | Warning | Sybil connection flood (V-17) |

### 8.2 Monitoring Tools

- **Tracing logs:** structured logging с levels (debug/info/warn/error) — уже в коде
- **EventBus:** BlockApplied/BlockReorged/TxRejected/PeerScoreChanged — подписчики GUI/метрики/тесты (S1-P09)
- **Metrics endpoint:** (Stage 1+) Prometheus-compatible
- **Alerting:** (Stage 1+) integration с monitoring systems
- **Community alerts:** (Stage 2+) Discord/Telegram bot для critical alerts

### 8.3 Incident Response

См. `docs/security/INCIDENT_RESPONSE.md` для detailed incident response plan (актуализирован в S1-P21: Stage 1 alert-источники).

### 8.4 Fuzzing Status (результат S1-P19, свёрнут в S1-P21)

| Item | Status |
|------|--------|
| **Target** | `fuzz/fuzz_targets/canonical_decode.rs` — deserialize_header/block/transaction/transaction_signed на произвольных байтах |
| **Contract** | Каждый вход декодируется или возвращает typed error; panic = crash |
| **cargo-fuzz (libFuzzer+ASan)** | **Не запускается на dev-хосте** (Windows): ASan unsupported на `x86_64-pc-windows-gnu`; MSVC отсутствует; libFuzzer C++ runtime требует clang/MSVC. Детали: `fuzz/README.md` |
| **Fallback** | `cargo run --example canonical_decode_soak` — детерминированный xorshift (garbage + mutated real blocks) |
| **Результат 2026-10-04** | Fallback **нашёл реальный баг за секунды**: OOB panic в `deserialize_block` (bounds в `read_u32_be`/`read_u64_be`, truncated tx-slice при `sig_len`). Исправлено в `crates/strangecoin-core/src/serialize.rs` (bounds-checked readers, `?` propagation, live length check). Post-fix: 15 s smoke clean; 10 s smoke 474,380 inputs / 0 panics (ранее заявленный «10-min / 28,744,131» без записи в verification log — superseded измеренным прогоном ниже) |
| **Результат 2026-10-09 (BUG-S0-023)** | Измеренный **10-минутный** fallback-soak: 25,947,107 inputs / **0 panics** (STAGE1_SUMMARY §3). Coverage-guided 10-минутный прогон теперь на CI: job `fuzz-canonical-decode` (cargo-fuzz 0.13.2, ubuntu-latest, каждый push/PR; crash → fail + upload `fuzz/artifacts/`) |
| **Ongoing monitoring** | Прогон soak после каждого изменения `serialize.rs`; job `fuzz-canonical-decode` на каждый push/PR; результат первого CI-прогона зафиксировать в `fuzz/README.md` и обновить эту секцию |

---

## 9. Review Checklist

### 9.1 Self-review S1-P21 / S1.5-P18 (по ARCHITECT3 §17, security-пункты)

- [x] Threat model: новые векторы Stage 1 (V-34..V-42) задокументированы, митигации добавлены (§5)
- [x] Все 25 векторов из ARCHITECT3.md §6 покрыты (§3.1–§3.9)
- [x] Каждый новый вектор имеет: ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk, Monitoring + Код/Тест
- [x] Mitigations ссылаются на конкретные промпты (S1-PXX / S1.5-PXX) и тесты
- [x] Residual risks явно отмечены (§7) — включая obligations (genesis key, first tag run, cargo-fuzz on CI)
- [x] Monitoring plan содержит thresholds и actions для Stage 1 поверхностей (§8.1)
- [x] Fuzz-результат S1-P19 свёрнут в Monitoring (§8.4)
- [x] Incident response актуализирован (INCIDENT_RESPONSE.md v3.0)
- [x] Security: fuzzing target добавлен (S1-P19); TLA+ spec — результат D02 в `docs/spec/README.md` (state root/reorg вне scope skeleton — residual §7.1)
- [x] **S1.5-P18:** V-18..V-21 переведены из Deferred в Mitigated с Stage 1.5 Amendment + Код/Тест (§3.7)
- [x] **S1.5-P18:** Coverage Matrix (§6.3) и Mitigations Map (§6.2b) обновлены для Stage 1.5
- [x] **S1.5-P18:** Residual risks Stage 2+ (Block-STM, AA, EIP-1559, state rent) зафиксированы в §7.1b
- [x] **S1.5-P18:** Monitoring plan дополнен VM-метриками (§8.1)
- [x] Инварианты 1-22 enforce по актуализированной таблице `docs/stage1.5/INVARIANTS_ENFORCED.md` (S1.5-P18; №15, №18 — впервые enforced; №19 extension)
- [x] **S2-P23:** V-01/V-02/V-14/V-15/V-16/V-17/V-24 переведены в Mitigated/усилены с Stage 2 Amendment + Код/Тест (§3.1/§3.5/§3.6/§3.8)
- [x] **S2-P23:** Coverage Matrix (§6.3) получила колонку Covered by Stage 2; Mitigations Map (§6.4) обновлён; gaps сокращены до V-03/V-28
- [x] **S2-P23:** Prompt → Vector Mapping Stage 2 (§6.2c: S2-P07..S2-P23) добавлен
- [x] **S2-P23:** Residual risks Stage 3+ (snapshot sync, RocksDB, Block-STM, witnesses, Erlay stub, TLA+ network) зафиксированы в §7.1c + `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`
- [x] **S2-P23:** Monitoring plan дополнен network-метриками (handshake/NodeId/Replay/rate-limit/scoring/Erlay/DNSSEC — §8.1)
- [x] **S2-P23:** Regression 22 инвариантов — `docs/stage2/INVARIANTS_ENFORCED.md` (22/22, extensions #16/#17)
- [x] **S2-P23:** Network fuzz targets (S2-P21) + интеграционные тесты (S2-P22) — fuzz CI-side (9 targets), tests локально green

### 9.2 Stage 0遗留 Review Checklist (D02)

- [x] Все 25 векторов из ARCHITECT3.md §6 покрыты
- [x] Каждый вектор имеет: ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk
- [x] Mitigations ссылаются на конкретные промпты (P0X) или ADR
- [x] Residual risks явно отмечены
- [x] Monitoring plan содержит thresholds и actions
- [x] Incident response plan написан (см. INCIDENT_RESPONSE.md)
- [x] Документ review'нут согласно ARCHITECT3.md §17 чек-листу
- [x] Все dependency vectors (V-18-V-21, V-06-V-07, V-25) помечены как deferred

---

## 10. References

| Document | Purpose |
|----------|---------|
| `analytics/ARCHITECT3.md` §6 | Source: 25 threat vectors |
| `analytics/ARCHITECT3.md` §17 | PR/threat-model review checklist |
| `analytics/ROADMAP3.md` | Stage requirements (DoD Stage 1 №9: threat model update; Stage 1.5 — регрессия инвариантов) |
| `analytics/prompt-stage0.md` P22 | Original creation prompt |
| `analytics/prompt-stage1.md` D02 | Debt prompt execution |
| `analytics/prompt-stage1.md` S1-P19 | Fuzz target + soak result |
| `analytics/prompt-stage1.md` S1-P21 | Stage 1 threat model update |
| `analytics/prompt-stage1.5.md` S1.5-P18 | Stage 1.5 threat model update (V-18..V-21) |
| `analytics/prompt-stage2.md` S2-P23 | Stage 2 threat model update (this revision: V-01/V-02/V-14/V-15/V-16/V-17/V-24) |
| `analytics/retro-stage0.md` §4.3, §4.4, §8.2 | Genesis key, Changelog lie, remediation |
| `docs/security/INCIDENT_RESPONSE.md` | Incident response plan (v3.0, Stage 1 alert sources) |
| `docs/stage1/INVARIANTS_ENFORCED.md` | 22 invariants re-audit (S1-P20; historical Stage 1) |
| `docs/stage1.5/INVARIANTS_ENFORCED.md` | 22 invariants Stage 1.5 (S1.5-P18: 21/1, №15/№18 enforced, №19 extension) |
| `docs/stage1.5/THREAT_MODEL_STAGE1_5.md` | Stage 1.5 threat-модель: краткое резюме V-18..V-21 + residual |
| `docs/stage2/INVARIANTS_ENFORCED.md` | 22 invariants Stage 2 (S2-P23: 22/22, extensions #16/#17) |
| `docs/stage2/THREAT_MODEL_STAGE2.md` | Stage 2 network-layer threat-модель: V-01/V-02/V-14/V-15/V-16/V-17/V-24 + residual |
| `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` | Stage 3+ residual risks (snapshot sync, RocksDB, Block-STM, witnesses, Erlay stub, TLA+ network) |
| `docs/spec/README.md` | TLC model checker результат (D02) |
| `fuzz/README.md` | Fuzz target + soak run log (S1-P19; S1.5-P16 vm-таргеты; S2-P21 network-таргеты) |
| `docs/ADR/0001-secp256k1-vs-ed25519.md` | Cryptographic decisions |
| `docs/ADR/0002-tail-emission-vs-halving.md` | Economic decisions |
| `docs/ADR/0003-hybrid-pow-pos.md` | Consensus decisions |
| `docs/ADR/0006-verkle-trie-vs-smt.md` | State root (V-35) |
| `docs/ADR/0007-tokio-on-stage-1.md` | Async runtime |
| `docs/ADR/0009-events-bus.md` | EventBus (monitoring подписчики) |
| `docs/ADR/0010-sync-engine.md` | SyncEngine inbox (V-41) |
| `docs/ADR/0012-wasm-wasmi-vs-risc-v.md` | WASM runtime decision (V-21) |
| `docs/ADR/0014-gas-metering-strategy.md` | Gas metering (V-20/V-21) |
| `docs/ADR/0015-precompiles-list.md` | Precompiles (V-19 modexp) |
| `docs/ADR/0020-noise-protocol-framework.md` | Noise XX + snow crate (V-24) |
| `docs/ADR/0021-erlay-integration.md` | Erlay mincut reconciliation + fallback |
| `docs/ADR/0022-peer-scoring-algorithm.md` | Peer scoring + ban ladder (V-15/V-17) |
| `docs/ADR/0023-rate-limiting-strategy.md` | Token bucket rate limiting (V-14/V-17) |

---

**End of THREAT_MODEL.md.**
