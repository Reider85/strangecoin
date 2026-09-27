# THREAT_MODEL.md — Strangecoin Threat Model (STRIDE)

**Версия:** 2.0 (D02)
**Дата:** 2026-09-28
**Статус:** Stage 0 — выполнен в D02 (debt prompt)
**Источник:** `analytics/ARCHITECT3.md` §6 (25 векторов атак) + `analytics/retro-stage0.md` (§4.3, §4.4, §8.2)
**Связанные документы:** `INCIDENT_RESPONSE.md`, `ARCHITECT3.md`, `ROADMAP3.md`

---

## 1. Введение

### 1.1 Scope

Настоящий документ описывает threat model для Strangecoin v1.0.0 (Stage 0).
Анализ охватывает следующие подсистемы:

- **Консенсус:** PoW валидация, difficulty retargeting, genesis, emission
- **Криптография:** secp256k1 подписи, blake3 хеши, каноническая сериализация
- **Сеть (P2P):** TCP соединения, gossip, sync, rate limiting
- **Хранение:** LevelDB, keystore (AES-256-GCM + PBKDF2)
- **Mempool:** валидация, eviction, лимиты
- **Кошелёк:** keystore шифрование, sign/verify
- **Сборка:** CI/CD, reproducible builds, signatures

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

## 4. Additional Stage 0-Specific Vectors

Помимо 25 основных векторов из ARCHITECT3.md §6,以下是在 Stage 0 специфичные векторы:

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
| **Residual Risk** | Низкий. chain_id isolation adequately mitigates cross-network contamination. |
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
| **Описание** | Приватный ключ генезисного холдера 1 000 000 000 монет выводится хэшированием публичной строки `"strangecoin-genesis-seed-2026"` (функция `genesis_keypair()` в `src/consensus/mod.rs`). Любой, кто видит исходный код, может воспроизвести ключ и потратить генезисные средства. |
| **Stage 0 Mitigation** | Осознанный компромисс для Stage 0/testnet. Комментарий в коде: «Real offline key will be used before mainnet freeze». На testnet/regtest генезисные средства не имеют реальной ценности. |
| **Stage** | Pre-mainnet (обязательно до mainnet freeze) |
| **Residual Risk** | **Высокий** до mainnet. Если mainnet запущен с этим ключом, средства украдены мгновенно. **Обязательство:** сгенерировать offline-ключ и заменить genesis_keypair() до mainnet freeze. |
| **Monitoring** | Нет ( ключ не используется в runtime, только при генезисе). Мониторинг genesis-транзакций в mainnet. |

---

#### V-32: Grant Blocks in Consensus Path

| Поле | Значение |
|------|----------|
| **ID** | V-32 |
| **Название** | Grant Blocks Bypass Consensus Rules |
| **STRIDE** | Elevation of Privilege / Tampering |
| **Описание** | Механизм `create_grant_block` (первичная эмиссия: 10000 монет «initial_wallet_address» первому кошельку) — это `is_coinbase: true` без подписи, прямая мутация `balances` в обход mempool и эмиссионных правил. В `validate_chain` захардкожено исключение `block.index != 1` (блок №1 выведен из проверки coinbase), а magic-строки `"genesis"`/`"coinbase"` в sender пропускают проверку баланса. |
| **Stage 0 Mitigation** | Генезисный блок продолжает валидироваться по `EXPECTED_GENESIS_HASH` (якорь P10). Grant-блоки работают только на regtest/testnet. |
| **Stage** | S1-P01 (consensus-санация: флаг `allow_grant_blocks`) |
| **Residual Risk** | **Средний.** Любой узел с grant-блоком нарушает инвариант №6 («блок не содержит наград сверх эмиссии»). На mainnet это критично — закрытие в S1-P01 обязательно до mainnet. |
| **Monitoring** | Мониторинг coinbase-транзакций с amount > block_reward_at_height; alert. |

---

#### V-33: Release Pipeline Never Executed

| Поле | Значение |
|------|----------|
| **ID** | V-33 |
| **Название** | Release Pipeline Never Run — Artifacts Non-Reproducible |
| **STRIDE** | Tampering |
| **Описание** | `.github/workflows/release.yml` содержит дефекты (build-job не объявляет `outputs.hashes`, 5 таргетов вместо 6) и ни разу не запускался (нет тегов `v*`). Артефакты reproducible builds (cosign, SLSA provenance) существуют только на бумаге. КГ P24 («при push тега запускается pipeline») не проверен. |
| **Stage 0 Mitigation** | Workflow и REPRODUCIBLE_BUILDS.md созданы (P24). Фикс — в D03 (PoC-тег `v0.0.0-rc1`). |
| **Stage** | D03 (PoC-тег) |
| **Residual Risk** | **Средний.** Пока pipeline не запущен, невозможно подтвердить воспроизводимость сборки. Пользователи не могут верифицировать бинарники. |
| **Monitoring** | Нет (pipeline не активен). Мониторинг GitHub Actions после fix. |

---

## 5. Mitigations Map

### 5.1 Prompt → Vector Mapping

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
| **P22** | All vectors (documentation, monitoring) |
| **P24** | V-22 (compromised build — reproducible builds, cosign, SLSA) |

### 5.2 Coverage Matrix

| STRIDE Category | Covered by P0X | Residual (deferred) |
|-----------------|----------------|---------------------|
| **Spoofing** | P05 (chain_id), P13 (peer limits) | V-06 (long-range → Stage 7), V-24 (MITM → Stage 2) |
| **Tampering** | P06 (serialization), P07 (txid), P08-P09 (difficulty/time) | V-07 (nothing-at-stake → Stage 7), V-18-V-21 (contracts → Stage 1.5+) |
| **Repudiation** | P07 (txid commitment) | — |
| **Information Disclosure** | P04 (keystore), P15 (config) | V-08, V-10 (MEV → Stage 5) |
| **Denial of Service** | P12 (size limits), P13 (rate limiting), P14 (mempool) | V-09 (censoring → Stage 7) |
| **Elevation of Privilege** | P05 (nonce), P10 (genesis) | V-05 (51% → monitoring), V-23 (key compromise → Stage 5 AA) |

### 5.3 Mitigations → Test Coverage Map

Каждый вектор привязан к конкретному тесту. Если тест отсутствует — помечен как **gap**.

| Вектор | Модуль/Функция | Тест | Статус |
|--------|---------------|------|--------|
| V-01 (Eclipse) | network/rate_limiter | `tests/network.rs::real_network_three_nodes` | ✅ D01 |
| V-02 (Partition) | network/protocol | `tests/network.rs::real_network_three_nodes` | ✅ D01 |
| V-03 (Selfish mining) | consensus/monitoring | gap: нет теста на orphan rate | ⚠️ gap |
| V-04 (Time-warp) | consensus (MTP, retarget) | `tests/time.rs::mtp_rejection`, `tests/time.rs::future_timestamp_rejection` | ✅ D01 |
| V-05 (51% attack) | consensus (difficulty) | `tests/pow.rs::mining_and_validation` | ✅ D01 |
| V-06 (Long-range) | N/A (PoS, Stage 7) | N/A | Deferred |
| V-07 (Nothing-at-stake) | N/A (PoS, Stage 7) | N/A | Deferred |
| V-08 (MEV frontrunning) | N/A (Stage 5) | N/A | Deferred |
| V-09 (MEV censoring) | N/A (Stage 7) | N/A | Deferred |
| V-10 (MEV sandwich) | N/A (Stage 5) | N/A | Deferred |
| V-11 (Cross-chain replay) | consensus (chain_id) | `tests/network.rs::network_id_rejection` (если создан) | ✅/gap |
| V-12 (Intra-chain replay) | mempool (nonce) | `tests/double_spend.rs::double_spend_rejected` | ✅ D01 |
| V-13 (OOM) | network (framing) | unit-тесты в `src/mempool/`, `src/network/` | ✅ Stage 0 |
| V-14 (Spam txs) | mempool (cap) | `tests/two_clients.rs` (нагрузочный) | ✅ D01 |
| V-15 (Invalid blocks) | peer_manager (ban) | gap: нет теста на ban после N invalid | ⚠️ gap |
| V-16 (DNS poisoning) | config (seeds) | gap: нет теста на DNS | ⚠️ gap |
| V-17 (Sybil) | peer_manager (limits) | gap: нет теста на connection limits | ⚠️ gap |
| V-18-V-21 (Contracts) | N/A (Stage 1.5+) | N/A | Deferred |
| V-22 (Compromised build) | CI/CD (P24) | gap: pipeline не запускался | ⚠️ gap (→ D03) |
| V-23 (Key compromise) | wallet (keystore) | unit-тесты wallet | ✅ Stage 0 |
| V-24 (MITM) | N/A (Stage 2) | N/A | Deferred |
| V-25 (Validator collusion) | N/A (Stage 7) | N/A | Deferred |
| V-26 (Testnet low-diff) | consensus (chain_id) | `tests/pow.rs::mining_and_validation` | ✅ D01 |
| V-27 (Keystore theft) | wallet (AES-GCM) | unit-тесты wallet | ✅ Stage 0 |
| V-28 (Config leakage) | config (P15) | gap: нет теста на secrets в config | ⚠️ gap |
| V-29 (Genesis manipulation) | consensus (hash) | `tests/common/mod.rs::genesis_validation` | ✅ D01 |
| V-30 (Nonce manipulation) | mempool (nonce) | `tests/double_spend.rs` | ✅ D01 |
| V-31 (Genesis key) | consensus (genesis_keypair) | gap: нет теста на offline key | ⚠️ gap (→ pre-mainnet) |
| V-32 (Grant blocks) | consensus (validate_chain) | `tests/emission.rs::coinbase_emission` | ✅ D01 |
| V-33 (Release pipeline) | CI/CD (P24) | gap: pipeline не запускался | ⚠️ gap (→ D03) |

**Итого gaps:** 7 векторов без тестов (V-03, V-15, V-16, V-17, V-28, V-31, V-33). Из них:
- V-03, V-15, V-16, V-17 — требуют интеграционных тестов (закрываются в D01/S1-P19)
- V-28 — требует unit-теста на config validation
- V-31 — требует offline-ключа (pre-mainnet obligation)
- V-33 — закрывается в D03 (PoC-тег)

---

## 6. Residual Risks

### 6.1 Risks Deferred to Later Stages

| Risk | Deferred To | Mitigation Status | Impact |
|------|-------------|-------------------|--------|
| MEV (V-08, V-09, V-10) | Stage 5 | No threshold encryption | High |
| Smart Contract Attacks (V-18-V-21) | Stage 1.5+ | No WASM contracts | N/A |
| PoS-Specific Attacks (V-06, V-07, V-25) | Stage 7 | No PoS | N/A |
| Network MITM (V-24) | Stage 2 | No Noise Protocol | High |
| Account Abstraction (V-23) | Stage 5 | No social recovery | Medium |
| Inclusion Lists (V-09) | Stage 7 | No censorship resistance | Medium |

### 6.2 Risks Accepted at Stage 0

| Risk | Reason | Mitigation |
|------|--------|------------|
| Selfish mining (V-03) | PoW inherent risk; no finality gadget | Monitoring + alert |
| 51% attack (V-05) | Small network; low hash rate | Monitoring + alert |
| Weak password (V-27) | User responsibility | PBKDF2 ≥210k iters |
| Plaintext P2P (V-24) | Noise Protocol deferred to Stage 2 | Accept; mitigate with monitoring |
| Genesis key from public string (V-31) | Testnet/regtest only; offline key before mainnet | **Obligation: offline key before mainnet freeze** |
| Grant blocks bypass consensus (V-32) | Testnet/regtest only; mainnet not launched | **Obligation: S1-P01 flag before mainnet** |
| Release pipeline non-functional (V-33) | Workflow exists but never run | **Obligation: D03 PoC-tag fix** |

### 6.3 Risk Acceptance签字

- [ ] Security team reviewed residual risks
- [ ] Accepted risks documented in ADR (if needed)
- [ ] Monitoring in place for all accepted risks
- [ ] Stage 1+ roadmap includes mitigation for high-impact deferred risks

---

## 7. Monitoring Plan

### 7.1 What to Monitor

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

### 7.2 Monitoring Tools

- **Tracing logs:** structured logging с levels (debug/info/warn/error)
- **Metrics endpoint:** (Stage 1+) Prometheus-compatible
- **Alerting:** (Stage 1+) integration с monitoring systems
- **Community alerts:** (Stage 2+) Discord/Telegram bot для critical alerts

### 7.3 Incident Response

См. `docs/security/INCIDENT_RESPONSE.md` для detailed incident response plan.

---

## 8. Review Checklist

Перед финализацией Threat Model:

- [ ] Все 25 векторов из ARCHITECT3.md §6 покрыты
- [ ] Каждый вектор имеет: ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk
- [ ] Mitigations ссылаются на конкретные промпты (P0X) или ADR
- [ ] Residual risks явно отмечены (что НЕ закрыто на Stage 0)
- [ ] Monitoring plan содержит thresholds и actions
- [ ] Incident response plan написан (см. INCIDENT_RESPONSE.md)
- [ ] Документ review'нут согласно ARCHITECT3.md §17 чек-листу
- [ ] Все dependency vectors (V-18-V-21, V-06-V-07, V-25) помечены как deferred

---

## 9. References

| Document | Purpose |
|----------|---------|
| `analytics/ARCHITECT3.md` §6 | Source: 25 threat vectors |
| `analytics/ROADMAP3.md` §Stage 0 Security | Stage 0 security requirements |
| `analytics/prompt-stage0.md` P22 | Original creation prompt |
| `analytics/prompt-stage1.md` D02 | Debt prompt execution |
| `analytics/retro-stage0.md` §4.3, §4.4, §8.2 | Genesis key, Changelog lie, remediation |
| `docs/security/INCIDENT_RESPONSE.md` | Incident response plan |
| `docs/ADR/0001-secp256k1-vs-ed25519.md` | Cryptographic decisions |
| `docs/ADR/0002-tail-emission-vs-halving.md` | Economic decisions |
| `docs/ADR/0003-hybrid-pow-pos.md` | Consensus decisions |

---

**End of THREAT_MODEL.md.**
