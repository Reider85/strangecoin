# ADR-0023: Rate Limiting Strategy

## Status

Accepted (2026-10-24) — S2-P04

## Context

Strangecoin's Stage 1 rate limiter (`src/network/rate_limiter.rs`) is a
sliding-window counter per `SocketAddr`: a total cap per 10 s window
plus per-class caps (`GetHeaders` 100, `GetBlocks` 100, `ChainGossip`
30, `Other` 30), with an immediate 5-minute in-memory ban on the first
exceed. ARCHITECT3 §3.6 mandates the token model («rate_limiter.rs:
токены на сообщения/сек от одного пира»); Stage 1 shipped windows
instead as an MVP. Invariant **#17** (docs/stage1.5/INVARIANTS_ENFORCED.md)
requires «лимит сообщений/сек от одного пира; нарушение → бан» and is
extended in Stage 2 with peer-scoring integration (S2-P23).

Forces at play:

- **Spam defense (V-14, V-17)** — Stage 2 adds gossip (INV/BLOCK/TX,
  S2-P13/S2-P14) whose legitimate traffic is bursty; an attacker
  flooding txs or invariants must be stopped per-peer without waiting
  for the scoring slow path (ADR-0022 evaluates on 60 s ticks).
- **Burst support without false bans** — a peer announcing a fresh
  block legitimately emits a tight burst (e.g. ~30 BLOCK messages while
  catching up a window of peers). Hard sliding-window caps punish
  exactly this legitimate pattern: ROADMAP3-era risk of banning honest
  peers applies to rate limits just as it does to scoring.
- **Per-type isolation** — an INV flood must not starve block relay for
  the same peer (or vice versa): independent sub-budgets per hot message
  type keep one spam channel from consuming the whole allowance.
- **Integration with peer scoring (ADR-0022)** — rate-limit bans are a
  fast, short, address-keyed reaction; repeated offenders must also
  accrue `protocol_violations` so the reputation layer (node_id-keyed,
  30-day persistent bans) eventually takes over. The two layers are
  complementary: this ADR is the fast path, ADR-0022 the slow path.
- **Hot-path cost** — the limiter runs on *every* inbound wire message;
  it must stay O(1) per check, allocation-free and lock-cheap, with no
  background timer threads.
- **Determinism (V-21 extension)** — the limiter is relay policy, not
  consensus state, but its refill arithmetic must be a pure function of
  (bucket state, elapsed time) so tests can drive it with a simulated
  clock (S2-P22 `net_helpers.rs`); no RNG anywhere.

## Decision

**We will replace the Stage 1 sliding-window limiter with a token-bucket
rate limiter — one aggregate bucket per peer plus per-message-type
sub-buckets for the gossip hot paths — with a violation counter
(rolling 60 s window): ≥5 violations ⇒ 5-minute fast-path ban; every
violation additionally increments the peer's `protocol_violations`
counter through the peer_manager API (ADR-0022 slow path).**

| Aspect | Decision |
|---|---|
| Algorithm | Token bucket with **lazy refill** — tokens restored arithmetically on each check from elapsed time; no background timers, no per-check allocation |
| Token cost | 1 token per inbound wire message (main bucket); sub-buckets charged in parallel for typed messages |
| Main bucket | capacity **1000** (burst), refill **100 tokens/s** (steady state) — covers all message types |
| Sub-buckets | token buckets per type: `INV` 100 burst / 10 per s; `BLOCK` 30 / 3 per s; `TX` 1000 / 100 per s; `GET_HEADERS` 100 / 10 per s; `GET_BLOCKS` 100 / 10 per s (see table below) |
| Exhaustion behavior | fail-closed: message refused (dropped), never queued; refusal = **1 violation** |
| Violation window | rolling 60 s; counter decays by expiry (violation timestamps within the window) |
| Ban threshold | **≥5 violations in 60 s ⇒ ban** (`config.toml: rate_limit_ban_threshold = 5`; window tunable) |
| Ban duration | 5 minutes (aligned with Stage 1; in-memory, per-address; recoverable) |
| Scoring integration | every violation → `peer_manager.record_rate_violation(...)` → `protocol_violations += 1` (ADR-0022 penalty channel; S2-P12 API) |
| Keying | `SocketAddr` (works pre-HELLO, before `node_id` is known); node_id-keyed consequences ride on ADR-0022 bans (S2-P09/S2-P12 identity arrives later in the handshake) |
| Tuning surface | all capacities, refill rates, thresholds, windows, ban duration in `config.toml` (`rate_limit_*` keys, S2-P16) |
| Determinism | refill = pure arithmetic on injected `now`; no RNG, no `SystemTime` inside the algorithm (clock injected at the edge) |

### Bucket parameters (initial)

| Bucket | Capacity (burst) | Refill / sec | Prompt notation | Covers |
|---|---|---|---|---|
| main | 1000 | 100 | «1000 tokens, 100 tokens/sec» | every inbound message |
| `INV` | 100 | 10 | «INV 100/10s» | S2-P13 inventory announces |
| `BLOCK` | 30 | 3 | «BLOCK 30/10s» | S2-P13/S2-P14 block relay |
| `TX` | 1000 | 100 | «TX 1000/10s» | S2-P13 tx relay + S2-P15 Erlay fallback |
| `GET_HEADERS` | 100 | 10 | Stage 1 carried over (100/10s) | headers-first sync pulls |
| `GET_BLOCKS` | 100 | 10 | Stage 1 carried over (100/10s) | block pull lane |

All other message types (HELLO, PING, ADDR, GET_DATA, …) draw from the
main bucket only. The Stage 1 `MessageClass` sliding windows
(`GetHeaders`/`GetBlocks`/`ChainGossip`/`Other`) are **replaced
wholesale** by this model in S2-P16: sync classes map onto the two sync
sub-buckets, `ChainGossip` (legacy `UPDATE_BLOCKCHAIN`, removed with
v0.8.6 cleanup in S2-P18) and `Other` map onto the main bucket.

### Worked examples

- **Legit block relay burst** — a peer emits 30 BLOCK messages in <1 s
  while catching up: the `BLOCK` sub-bucket capacity (30) absorbs the
  entire burst with zero violations; steady-state relay continues at
  3 BLOCK/s. No false ban.
- **INV spammer (1000 INV/s)** — `INV` bucket absorbs the first 100;
  every subsequent INV is refused ⇒ 5 violations accumulate within the
  first second ⇒ 5-minute ban almost immediately. Desired: this is
  exactly threat V-17/V-14.
- **Slight over-burst** — a legitimate peer emitting 105 INV within a
  few seconds exhausts the bucket and accrues a handful of refusals; it
  may hit the 5-minute fast-path ban. Cost is bounded: the ban is short
  and in-memory, and ≤5 `protocol_violations` subtract at most 0.25
  from its ADR-0022 score (still far above the persistent threshold).
  Mitigations: generous burst capacities, tunable threshold, and the
  ban being the recoverable fast path rather than a reputation event.
- **Main-bucket saturation** — mixed traffic above 100 msgs/s
  sustained exhausts the main bucket even when each type stays within
  its sub-bucket; refusals + violations proceed identically. Aggregate
  cap holds regardless of type-mixing games.

### Integration with peer scoring (ADR-0022)

```
inbound message
   │
   ▼
rate limiter (this ADR — fast path, address-keyed, ms reaction)
   │ refuse ──▶ violation++ (rolling 60 s)
   │               │
   │               ├─▶ ≥5 in 60 s ─▶ ban 5 min (in-memory)
   │               │
   │               └─▶ peer_manager.record_rate_violation()
   │                        └─▶ protocol_violations += 1  (ADR-0022)
   │                              └─▶ score drops on next 60 s tick;
   │                                  Warn → TempBan → PersistentBan
   │                                  (node_id-keyed, network.json)
   └─ accept ──▶ protocol handlers
```

- The fast path reacts in milliseconds; the slow path (ADR-0022)
  converts repetition into reputation damage and eventually 30-day
  persistent bans that survive restarts.
- Each *refused message* is one violation (not one ban event), so a
  sustained flooder is graduated into the scoring system immediately,
  while a single accidental burst produces at most a short ban.
- This closes the ADR-0022 «Neutral» integration point: rate violations
  are the primary feed of `protocol_violations`.

### Determinism notes

1. Refill is lazy arithmetic: `tokens = min(capacity, tokens +
   refill_per_sec × elapsed)` — a pure function of bucket state and the
   injected clock reading; identical inputs ⇒ identical verdicts.
2. No `thread_rng` / `OsRng`; no background refill threads.
3. Wall clock is injected at the API edge (`check(now, ...)`); tests
   (S2-P22) drive it with a simulated clock. Rate limiting is relay
   policy — it never enters block/tx validation, so
   `deterministic_replay` is unaffected (same reasoning as ADR-0022).

## Consequences

### Positive

- **Burst support** — legitimate tight bursts (block relay, catch-up
  sync) fit inside bucket capacity and never trip the limiter; the
  steady-state rate is still enforced over time. Directly removes the
  Stage 1 false-positive pattern (hard window caps).
- **Per-type isolation** — an INV or TX flood exhausts only its own
  sub-bucket; block relay and sync keep their budgets for the same
  peer, and one spam channel cannot starve honest traffic classes.
- **Fast + slow defense pair** — millisecond address-keyed ban (this
  ADR) complements the 60-second-tick node_id-keyed reputation ladder
  (ADR-0022); repeat offenders are permanently removed, one-shot
  abusers are merely delayed 5 minutes.
- **O(1) hot path** — lazy refill, no timers, no allocations per check;
  per-peer state is a handful of counters (fits the lock-ordering
  constraints: limiter state never held across `BlockchainFacade`
  calls).
- **Tunable** — every capacity, rate, threshold and duration is
  operator-adjustable in `config.toml` without recompilation.
- **Invariant #17 stays enforced** — limits messages/sec per peer;
  violation ⇒ ban; Stage 2 extends it with scoring integration (S2-P23
  updates INVARIANTS_ENFORCED.md accordingly).

### Negative

- **Per-peer memory overhead** — each peer carries the main bucket
  (tokens, last-refill timestamp) plus one sub-bucket record per typed
  class (~5 × 16–32 bytes) plus the violation ring (~60 s of
  timestamps or a decayed counter): on the order of 200–300 bytes × N
  peers. Bounded by connection limits (S2-P12 `max_connections`).
- **Address-keyed fast path** — bans and buckets are keyed by
  `SocketAddr` because `node_id` is unknown before the S2-P09 auth
  round. An honest peer behind the same NAT/IP reusing the banned
  address waits out the (short) ban; conversely an attacker rotates
  source ports to reset fast-path state. Mitigation: ADR-0022
  node_id-keyed bans catch repeat offenders regardless of address;
  full identity-keying of the limiter is deferred until node_id is
  available at check time (post-S2-P09).
- **Boundary over-burst can cost a short ban** — see worked example 3;
  accepted with the bounded-cost mitigations above.
- **Violation-per-refusal counting is aggressive** — under sustained
  flood the 5-violation threshold is reached almost instantly (that is
  the point), but it also means the threshold provides little
  discrimination between "moderately over" and "fully malicious";
  discrimination is delegated to the scoring layer (ADR-0022), which
  weighs violations against quality factors over longer horizons.

### Neutral

- **Stage 1 sliding windows are removed, not kept in parallel** — one
  mechanism, one state machine; `MessageClass` survives only as the
  type→sub-bucket mapping until S2-P18 drops the legacy classes
  entirely.
- **Relay policy, not consensus** — no SCIP / activation height needed
  pre-mainnet-freeze; parameter changes are config-only.
- **Integration surface is fixed here, implemented in S2-P16** — the
  peer_manager API (`record_rate_violation`) and `config.toml` key
  names are defined by this ADR; S2-P16 provides the implementation and
  S2-P22 the end-to-end spammer test (`net_rate_limit_integration.rs`).
- **Compact blocks / Erlay interplay** — BIP 152 fill-up (S2-P14) and
  Erlay full-relay fallback (ADR-0021) deliberately sit inside the
  generous TX/BLOCK budgets (1000/10s, 30/10s); reconciliation INV
  traffic rides the INV budget. Reconciliation rounds are periodic
  (60 s), not burst-generating.

## Alternatives Considered

### Alternative 1: Global (aggregate) rate limit

- Pros: single counter for the whole node; trivial state; bounds
  worst-case total ingress regardless of peer count.
- Cons: no per-peer DoS defense — one flooder consumes the entire
  budget and starves every honest peer (the attacker does the work of
  many). Sybil trivially scales the attack (V-17): N fake peers ×
  global/N each still degrade everyone.
- Why not chosen: per-peer isolation is the whole point of invariant
  #17; a global limiter is at best a supplementary node-wide safety
  valve, not a peer-defense strategy.

### Alternative 2: Leaky bucket

- Pros: similar steady-state enforcement to a token bucket; smooths
  output rather than allowing bursts.
- Cons: burst behavior is harder to reason about and to tune (queue
  depth semantics vs token count); "hard" leaky buckets without a
  queue reject the burst head — exactly the legitimate-burst false
  positive this design avoids; industry practice (Bitcoin Core,
  libp2p) converged on token buckets for the same reason.
- Why not chosen: token bucket expresses "burst capacity + steady
  rate" directly in two intuitive parameters.

### Alternative 3: Fixed window counters

- Pros: cheapest possible state (one counter + window boundary per
  peer); O(1).
- Cons: boundary burst — a sender may push 2× the intended rate across
  a window edge (full bucket at the end of window N, full bucket again
  at the start of window N+1); either the rate must be halved to
  compensate (punishing honest steady traffic) or the burst hole is
  accepted (undermining the limit).
- Why not chosen: the boundary doubling is a known structural flaw;
  token buckets have no window edges.

### Alternative 4: Sliding window log (precise timestamps)

- Pros: mathematically precise rate over any interval; no boundary
  artifact.
- Cons: memory-heavy — every message timestamp must be retained for the
  window (a 1000-msg/s peer stores 1000+ timestamps); allocation and
  cache churn on the hottest path in the node; precision is unneeded
  when a token bucket's ±1-message error is irrelevant at these rates.
- Why not chosen: cost on the hot path buys nothing operationally.

### Alternative 5: Keep Stage 1 sliding windows

- Pros: already implemented and battle-tested in Stage 1; no rewrite
  risk; invariant #17 already green.
- Cons: hard caps punish legitimate bursts (block relay) by design —
  the ROADMAP3-style "ban legitimate peers" risk in a different form;
  no burst-vs-steady-state distinction; coarse per-class model does not
  cover the new Stage 2 gossip types (INV/BLOCK/TX); ARCHITECT3 §3.6
  explicitly specifies the token model.
- Why not chosen: Stage 2's bursty gossip traffic makes the window
  model's false positives operational, and the architecture document
  mandates tokens; S2-P16 replaces it wholesale.

## Related

- ARCHITECT3 §3.6 (network/p2p — «rate_limiter.rs: токены на сообщения/
  сек от одного пира»), §6 threats V-14 (spam txs), V-17 (Sybil)
- Invariant #17 — docs/stage1.5/INVARIANTS_ENFORCED.md («Rate limiting:
  лимит сообщений/сек от одного пира; нарушение → бан»); Stage 2
  extension recorded in S2-P23
- ADR-0022 (peer scoring — rate violations feed `protocol_violations`;
  fast path / slow path pairing; ban-threshold interplay)
- ADR-0020 (Noise — frames are rate-checked after decryption/framing;
  encrypted transport does not bypass the limiter)
- ADR-0021 (Erlay — reconciliation INV traffic rides the INV budget;
  fallback TX bursts fit the TX budget)
- Stage 2 prompts: S2-P13 (INV/GET_DATA/TX/BLOCK gossip — the hot
  types budgeted here), S2-P14 (compact blocks — BLOCK budget),
  S2-P16 (implementation: `rate_limiter.rs` rewrite, `config.toml`
  `rate_limit_*` keys, `peer_manager.record_rate_violation` API),
  S2-P18 (legacy class removal — `ChainGossip`/`Other` die here),
  S2-P22 (`net_rate_limit_integration.rs`: 1000 INV/s → banned after 5
  violations / 60 s)
- Stage 2 Determinism Checklist (AGENTS.md) — lazy-refill arithmetic,
  injected clock
- docs/security/THREAT_MODEL.md — V-14 / V-17 mitigations (updated in
  S2-P23)

## Renumbering note

ROADMAP3 originally planned this ADR as ADR-0019 (fourth of the planned
ADR-0016..0019 block for Stage 2). Stage 1.5 FBP07 occupied ADR-0016 for
«Receipts Root in Block Header», so all Stage 2 ADRs were renumbered to
ADR-0020..0023 (noise / erlay / peer-scoring / rate-limiting). See the
full renumbering note in ADR-0020; the decision is recorded in the
S2-P01 commit message.
