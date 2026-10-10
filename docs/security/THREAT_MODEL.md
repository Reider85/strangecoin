# THREAT_MODEL.md — Strangecoin Threat Model (STRIDE)

**Версия:** 3.0 (S1-P21)
**Дата:** 2026-10-04
**Статус:** Stage 1 — актуализирован в S1-P21 (Stage 0 — выполнен в D02)
**Источник:** `analytics/ARCHITECT3.md` §6 (25 векторов атак) + `analytics/retro-stage0.md` (§4.3, §4.4, §8.2) + `analytics/prompt-stage1.md` S1-P21 (векторы Stage 1)
**Связанные документы:** `INCIDENT_RESPONSE.md`, `ARCHITECT3.md`, `ROADMAP3.md`, `docs/stage1/INVARIANTS_ENFORCED.md`, `fuzz/README.md`

---

## 1. Введение

### 1.1 Scope

Настоящий документ описывает threat model для Strangecoin (Stage 0 — v1.0.0, Stage 1 — v1.1.0-stage1).
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

### 1.2 Assumptions

1. **Сеть:** асинхронная, доставка сообщений не гарантирована, задержки переменны
2. **Узлы:** некоторые узлы могут быть враждебными (Byzantine fault tolerant)
3. **Каналы:** TCP соединения перехватываются (plaintext на Stage 0, Noise Protocol с Stage 2)
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
| **Stage** | P13 |
| **Residual Risk** | Средний. На Stage 0 нет Noise Protocol (P24), поэтому MITM возможен. Peer diversity ограничена отсутствием DNSSEC. |
| **Monitoring** | Мониторинг количества активных пиров; alert при падении ниже 8 outgoing; логирование новых/потерянных соединений. |

---

#### V-02: Partition Attack

| Поле | Значение |
|------|----------|
| **ID** | V-02 |
| **Название** | Partition Attack |
| **STRIDE** | Denial of Service |
| **Описание** | Разделение сети на несколько изолированных частей. Атакующий перехватывает или блокирует сообщения между группами узлов, создавая два или более изолированных консенсуса. |
| **Stage 0 Mitigation** | Cross-checking chain tips с multiple peers; `sync_engine` запрашивает headers у ≥3 пиров; alert при расхождении chain tip > 1 блока. |
| **Stage** | P13 |
| **Residual Risk** | Средний. На Stage 0 без Noise Protocol возможен targeted MITM для partition. Централизация seed nodes — residual risk. |
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
| **Stage 0 Mitigation** | `chain_id` в каждой tx (P05); mainnet=1, testnet=2, regtest=3; инвариант #10: `tx.chain_id != current_chain_id() → reject`. |
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
| **Stage 0 Mitigation** | Length-prefixed framing с проверкой ДО аллокации (P12); `MAX_MESSAGE_SIZE = 32MB`, `MAX_BLOCK_SIZE = 4MB`, `MAX_TX_SIZE = 256KB`; инвариант #16. |
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
| **Stage** | P13, P14 |
| **Residual Risk** | Низкий. Rate limiting + mempool cap adequately mitigate spam. |
| **Monitoring** | Мониторинг mempool size; alert при > 80% capacity; мониторинг rejected tx rate per peer. |

---

#### V-15: DoS Invalid Blocks

| Поле | Значение |
|------|----------|
| **ID** | V-15 |
| **Название** | DoS: Invalid Blocks |
| **STRIDE** | Denial of Service |
| **Описание** | Пиров атакует invalid блоками. Каждый invalid block требует expensive validation (signature verification, PoW check). Это перегружает CPU узла. |
| **Stage 0 Mitigation** | `peer_manager` scoring; ban после N invalid blocks; `chain_selector` не применяет блоки до валидации (P13). |
| **Stage** | P13 |
| **Residual Risk** | Низкий. Ban policy + scoring adequately mitigate invalid block spam. |
| **Monitoring** | Alert при ban; мониторинг invalid block rate per peer. |

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
| **Stage** | P01 (seeds configuration) |
| **Residual Risk** | Средний. Hard-coded IPs help, но не все种子节点 доступны по IP. DNSSEC не везде. |
| **Monitoring** | Мониторинг seed node availability; alert при неудачных подключениях к seeds. |

---

#### V-17: Sybil Attack

| Поле | Значение |
|------|----------|
| **ID** | V-17 |
| **Название** | Sybil Attack |
| **STRIDE** | Spoofing |
| **Описание** | Атакующий создает множество фейковых пиров для: (1) eclipse attack, (2) увеличения влияния на gossip, (3) перегрузки peer management. |
| **Stage 0 Mitigation** | `peer_manager` scoring; limit on connections per IP range; ban on protocol violations (P13); connection limits (max_peers config). |
| **Stage** | P13 |
| **Residual Risk** | Средний. IP-based limits не идеальны (NAT, VPN, Tor). Более продвинутыеmeasures (proof of work for connections) — Stage 2+. |
| **Monitoring** | Мониторинг connections per IP; alert при > 3 connections с одного /24 subnet. |

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
| **Stage 0 Mitigation** | `reentrant: bool` в `ExecutionContext`; `nonReentrant` modifier pattern; `gas_limit` enforcement. **Deferred: WASM смарт-контракты на Stage 1.5.** |
| **Stage** | Stage 1.5 |
| **Residual Risk** | N/A на Stage 0 (нет смарт-контрактов). |
| **Monitoring** | N/A на Stage 0. |

---

#### V-19: Integer Overflow (Smart Contracts)

| Поле | Значение |
|------|----------|
| **ID** | V-19 |
| **Название** | Integer Overflow (Smart Contracts) |
| **STRIDE** | Tampering |
| **Описание** | Переполнение в арифметике смарт-контракта. Например, `uint256 balance = 0 - 1` → massive balance. Это позволяет: (1) создать токены из ничего, (2) обойти проверки баланса. |
| **Stage 0 Mitigation** | Checked arithmetic (Rust default); `SafeMath`-style precompiles; gas cost for arithmetic ops. **Deferred: WASM смарт-контракты на Stage 1.5.** |
| **Stage** | Stage 1.5 |
| **Residual Risk** | N/A на Stage 0 (нет смарт-контрактов). Rust prevents overflow в native code. |
| **Monitoring** | N/A на Stage 0. |

---

#### V-20: Storage Exhaustion

| Поле | Значение |
|------|----------|
| **ID** | V-20 |
| **Название** | Storage Exhaustion |
| **STRIDE** | Denial of Service |
| **Описание** | Контракт заполняет весь state, делая узел недоступным. Каждый write стоит gas, но если gas limit высокий, контракт может заполнить terabytes. |
| **Stage 0 Mitigation** | `max_storage_per_contract`; `state_rent` (Stage 3+); gas cost for storage writes. **Deferred: WASM смарт-контракты на Stage 1.5.** |
| **Stage** | Stage 1.5+ |
| **Residual Risk** | N/A на Stage 0 (нет смарт-контрактов). |
| **Monitoring** | N/A на Stage 0. |

---

#### V-21: Code Injection (Malicious WASM)

| Поле | Значение |
|------|----------|
| **ID** | V-21 |
| **Название** | Code Injection (Malicious WASM) |
| **STRIDE** | Tampering / Elevation of Privilege |
| **Описание** | Загрузка вредоносного WASM-модуля, который: (1) выполняет произвольный код, (2) читает/пишет state, (3) обходит gas metering. |
| **Stage 0 Mitigation** | wasmi sandbox (no I/O by default); host functions whitelist; gas metering prevents infinite loops. **Deferred: WASM смарт-контракты на Stage 1.5.** |
| **Stage** | Stage 1.5 |
| **Residual Risk** | N/A на Stage 0 (нет смарт-контрактов). |
| **Monitoring** | N/A на Stage 0. |

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
| **Stage** | Stage 2 |
| **Residual Risk** | Высокий на Stage 0. Все P2P трафик plaintext. MITM возможен в любой момент. |
| **Monitoring** | Мониторинг unexpected disconnects; alert при подозрительных patterns. |

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
| **Residual Risk** | **Высокий** для любой публичной цепочки, запущенной с текущим testnet-генезисом. Residual: genesis key derived from `"strangecoin-genesis-seed-2026"`; **BURNED** (pubkey остался в testnet `genesis.json`); replacement via **SCIP-0001 mandatory before block 1 of mainnet**. Код больше не хранит секрет; замена ключа — операционный шаг (offline key → новый pubkey → новый `EXPECTED_GENESIS_HASH`). |
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

### 6.3 Coverage Matrix

| STRIDE Category | Covered by Stage 0 | Covered by Stage 1 | Residual (deferred) |
|-----------------|--------------------|--------------------|---------------------|
| **Spoofing** | P05 (chain_id), P13 (peer limits) | S1-P14 (network_id HELLO), S1-P15 (bech32 HRP) | V-06 (long-range → Stage 7), V-24 (MITM → Stage 2) |
| **Tampering** | P06 (serialization), P07 (txid), P08-P09 (difficulty/time) | S1-P06 (state_root), S1-P08 (tx_root), S1-P05 (consensus_version), S1-P16 (headers) | V-07 (nothing-at-stake → Stage 7), V-18-V-21 (contracts → Stage 1.5+) |
| **Repudiation** | P07 (txid commitment) | — | — |
| **Information Disclosure** | P04 (keystore), P15 (config) | S1-P07 (witness minimality) | V-08, V-10 (MEV → Stage 5) |
| **Denial of Service** | P12 (size limits), P13 (rate limiting), P14 (mempool) | S1-P17 (RBF limits), S1-P18 (inbox bounded) | V-09 (censoring → Stage 7) |
| **Elevation of Privilege** | P05 (nonce), P10 (genesis) | S1-P01 (grant flag), S1-P05 (version enforce) | V-05 (51% → monitoring), V-23 (key compromise → Stage 5 AA) |

### 6.4 Mitigations → Test Coverage Map

Каждый вектор привязан к конкретному тесту. Если тест отсутствует — помечен как **gap**.

| Вектор | Модуль/Функция | Тест | Статус |
|--------|---------------|------|--------|
| V-01 (Eclipse) | network/rate_limiter | `tests/network.rs::real_network_three_nodes` | ✅ D01 |
| V-02 (Partition) | network/protocol | `tests/network.rs::real_network_three_nodes` | ✅ D01 |
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
| V-14 (Spam txs) | mempool (cap) | `tests/two_clients.rs` (нагрузочный) | ✅ D01 |
| V-15 (Invalid blocks) | peer_manager (ban) + HeaderCache + block_executor | `tests/block_executor.rs` (reject-матрица), `tests/sync_headers.rs` (bad header не доходит до tip); ban-after-N — частично | ⚠️ частично |
| V-16 (DNS poisoning) | config (seeds) | gap: нет теста на DNS | ⚠️ gap |
| V-17 (Sybil) | peer_manager (limits) | gap: нет теста на connection limits | ⚠️ gap |
| V-18-V-21 (Contracts) | N/A (Stage 1.5+) | N/A | Deferred |
| V-22 (Compromised build) | CI/CD (P24) | gap: GitHub Actions run не верифицирован | ⚠️ gap (→ first v* tag) |
| V-23 (Key compromise) | wallet (keystore) | unit-тесты wallet | ✅ Stage 0 |
| V-24 (MITM) | N/A (Stage 2) | N/A | Deferred |
| V-25 (Validator collusion) | N/A (Stage 7) | N/A | Deferred |
| V-26 (Testnet low-diff) | consensus (chain_id) | `tests/pow.rs::mining_and_validation` | ✅ D01 |
| V-27 (Keystore theft) | wallet (AES-GCM) | unit-тесты wallet | ✅ Stage 0 |
| V-28 (Config leakage) | config (P15) | gap: нет теста на secrets в config | ⚠️ gap |
| V-29 (Genesis manipulation) | consensus (hash) | `tests/common/mod.rs::genesis_validation`; panic-guard при mismatch | ✅ D01 |
| V-30 (Nonce manipulation) | mempool (nonce) | `tests/double_spend.rs` | ✅ D01 |
| V-31 (Genesis key) | consensus (load_genesis pubkey-only) + SCIP-0001 | `tests/genesis_key.rs` (hash без секрета в коде, pubkey-only genesis.json, seed regression-guard); offline key — pre-mainnet ops (SCIP-0001) | ✅ code (S1.5-P01); ⚠️ ops (→ mainnet freeze) |
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

**Итого gaps:** 5 векторов без полных тестов (V-03, V-15, V-16, V-17, V-28). Из них:
- V-03, V-16, V-17, V-28 — требуют интеграционных/unit-тестов (не закрыты Stage 1)
- V-15 — частично закрыт reject-тестами block_executor/sync; ban-after-N остаётся gap
- ~~V-31~~ — **closed code-side** (S1.5-P01): тест genesis без секрета в коде; residual ops (offline key) — SCIP-0001 pre-mainnet
- ~~V-22/V-33~~ — **closed** (BUG-S1-001, 2026-10-10): первый release-run на GitHub Actions верифицирован (run 38071514479)
- ~~V-35 zero-root вектор (3)~~ — **closed** (BUG-S1-002, 2026-10-11): zero-root opt-out удалён (SCIP-0002); mainnet/testnet обязателен commitment
- Новые V-34..V-42 — все с тестами, gaps нет

---

## 7. Residual Risks

### 7.1 Risks Deferred to Later Stages

| Risk | Deferred To | Mitigation Status | Impact |
|------|-------------|-------------------|--------|
| MEV (V-08, V-09, V-10) | Stage 5 | No threshold encryption | High |
| Smart Contract Attacks (V-18-V-21) | Stage 1.5+ | No WASM contracts | N/A |
| PoS-Specific Attacks (V-06, V-07, V-25) | Stage 7 | No PoS | N/A |
| Network MITM (V-24) | Stage 2 | No Noise Protocol | High |
| Account Abstraction (V-23) | Stage 5 | No social recovery | Medium |
| Inclusion Lists (V-09) | Stage 7 | No censorship resistance | Medium |
| Light client / SPV (V-36 wire) | Stage 2+ | Witness API только в ядре, не в протоколе | Medium |
| TLA+ model: Verkle/state root/reorg | Stage 1 (spec update) | `docs/spec/README.md`: свойства state root/reorg — вне scope skeleton | Low |
| Gossip/Noise/Erlay (усиливают V-01, V-34, V-41) | Stage 2 | Планируется | Medium |

### 7.2 Risks Accepted (Stage 0 + Stage 1)

| Risk | Reason | Mitigation |
|------|--------|------------|
| Selfish mining (V-03) | PoW inherent risk; no finality gadget | Monitoring + alert |
| 51% attack (V-05) | Small network; low hash rate | Monitoring + alert |
| Testnet 51% (V-26) | Low testnet difficulty by design (fast blocks); chain_id protects mainnet | **Obligation: retarget hardening via SCIP post-mainnet-freeze + Stage 6 audit; testnet residual accepted** |
| Weak password (V-27) | User responsibility | PBKDF2 ≥210k iters |
| Plaintext P2P (V-24) | Noise Protocol deferred to Stage 2 | Accept; mitigate with monitoring |
| Genesis key from public string (V-31) | Testnet/regtest only; secret removed from code (S1.5-P01); pubkey remains burned | **Obligation: offline key + SCIP-0001 before mainnet freeze** |
| Grant blocks (V-32) | Flag-gated (S1-P01); must stay off on mainnet | Flag default false + tests |
| Release pipeline (V-33) | ~~Workflow fixed (D03); GitHub Actions run pending~~ **closed 2026-10-10 (BUG-S1-001)**: run 38071514479 green, cosign/SLSA verified | First-run evidence: REPRODUCIBLE_BUILDS.md §Verified Release Runs |
| cargo-fuzz не запускается на dev-хосте (Windows, нет MSVC/ASan) | ASan unsupported on `x86_64-pc-windows-gnu`; no MSVC; libFuzzer needs clang/MSVC | Fallback soak runner (S1-P19) нашёл и закрыл OOB-баг; измеренный 10-min soak clean (2026-10-09). **Obligation закрыта (BUG-S0-023, 2026-10-09): job `fuzz-canonical-decode` — cargo-fuzz 600s на Linux CI, каждый push/PR**; residual — результат первого CI-прогона |
| Zero state_root tolerated (opt-in no commitment) | Явное поведение для совместимости | Узлы с enforced commitments отклоняют zero root; документировано |
| RBF при fee=0 (feerate = proxy по размеру) | Fee market — Stage 5 | RBF_MIN_DELTA + MAX_RBF_REPLACEMENTS + rate limiter |
| DNS poisoning (V-16) | Stage 0 residual | Hard-coded seeds; DNSSEC — Stage 2 |

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

### 9.1 Self-review S1-P21 (по ARCHITECT3 §17, security-пункты)

- [x] Threat model: новые векторы Stage 1 (V-34..V-42) задокументированы, митигации добавлены (§5)
- [x] Все 25 векторов из ARCHITECT3.md §6 покрыты (§3.1–§3.9)
- [x] Каждый новый вектор имеет: ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk, Monitoring + Код/Тест
- [x] Mitigations ссылаются на конкретные промпты (S1-PXX) и тесты
- [x] Residual risks явно отмечены (§7) — включая obligations (genesis key, first tag run, cargo-fuzz on CI)
- [x] Monitoring plan содержит thresholds и actions для Stage 1 поверхностей (§8.1)
- [x] Fuzz-результат S1-P19 свёрнут в Monitoring (§8.4)
- [x] Incident response актуализирован (INCIDENT_RESPONSE.md v3.0)
- [x] Security: fuzzing target добавлен (S1-P19); TLA+ spec — результат D02 в `docs/spec/README.md` (state root/reorg вне scope skeleton — residual §7.1)
- [x] Все dependency vectors (V-18-V-21, V-06-V-07, V-25) помечены как deferred
- [x] Инварианты 1-22 enforce по актуализированной таблице `docs/stage1/INVARIANTS_ENFORCED.md` (S1-P20; №19, №21 — впервые enforced)

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
| `analytics/ROADMAP3.md` | Stage requirements (DoD Stage 1 №9: threat model update) |
| `analytics/prompt-stage0.md` P22 | Original creation prompt |
| `analytics/prompt-stage1.md` D02 | Debt prompt execution |
| `analytics/prompt-stage1.md` S1-P19 | Fuzz target + soak result |
| `analytics/prompt-stage1.md` S1-P21 | Stage 1 threat model update (this revision) |
| `analytics/retro-stage0.md` §4.3, §4.4, §8.2 | Genesis key, Changelog lie, remediation |
| `docs/security/INCIDENT_RESPONSE.md` | Incident response plan (v3.0, Stage 1 alert sources) |
| `docs/stage1/INVARIANTS_ENFORCED.md` | 22 invariants re-audit (S1-P20) |
| `docs/spec/README.md` | TLC model checker результат (D02) |
| `fuzz/README.md` | Fuzz target + soak run log (S1-P19) |
| `docs/ADR/0001-secp256k1-vs-ed25519.md` | Cryptographic decisions |
| `docs/ADR/0002-tail-emission-vs-halving.md` | Economic decisions |
| `docs/ADR/0003-hybrid-pow-pos.md` | Consensus decisions |
| `docs/ADR/0006-verkle-trie-vs-smt.md` | State root (V-35) |
| `docs/ADR/0007-tokio-on-stage-1.md` | Async runtime |
| `docs/ADR/0009-events-bus.md` | EventBus (monitoring подписчики) |
| `docs/ADR/0010-sync-engine.md` | SyncEngine inbox (V-41) |

---

**End of THREAT_MODEL.md.**
