# ADR-0022: Peer Scoring Algorithm

## Status

Accepted (2026-10-24) — S2-P03

## Context

Strangecoin's Stage 1 network has no peer scoring at all: the only
defense is the in-memory rate limiter (`src/network/rate_limiter.rs`,
5-min ban on message-rate exceed), which resets on restart and scores
nothing. ARCHITECT3 §3.6 mandates the `peer_store.rs` / `peer_manager.rs`
/ `connection_pool.rs` split (ARCHITECT.md §11.6) with scoring factors
«latency, invalid blocks/txs sent, protocol violations, uptime»; §6
threats require it directly:

- **V-15 (Invalid blocks DoS)** — a peer feeding invalid blocks is only
  rejected per-block today; nothing stops it from repeating forever.
- **V-17 (Sybil flooding)** — fake peers multiply; without accumulated
  reputation there is no way to tell a noisy new peer from a stable one.
- **V-1 (Eclipse)** — `peer_manager` scoring is the listed mitigation for
  preferring diverse, healthy peers over attacker-controlled ones.

Forces at play:

- **ROADMAP3 risk: «Peer scoring может ban legitimate peers»** — a naive
  rule (ban on first violation) false-positives on peers behind slow
  links or lossy mobile networks. Mitigation must be structural, not
  aspirational: tunable thresholds + a factor that only *grades*
  (latency, uptime) and never bans on its own.
- **Persistent bans must survive restart** — ROADMAP3 Этап 2 task: «бан
  временный (expire) + persistent (в `network.json`)». A peer banned for
  30 days must not reappear after the operator restarts the node.
- **Stable peer identity** — bans must be keyed by something that does
  not change. Per ADR-0020 / S2-P09, `node_id = blake3(Ed25519 pubkey)`
  is a long-term static key stored in the keystore — it does not rotate,
  so a persistent ban keyed by `node_id` does not go stale (a peer cannot
  escape a ban by changing its IP; it would need a new keypair, i.e. a
  new identity — at which point it is a new peer subject to the default
  untrusted baseline).
- **Integrations** — rate limiting (ADR-0023 / S2-P16) must feed
  violations into the score; Erlay reconciliation (ADR-0021) surfaces
  sustained mismatch rate as a peer-quality signal; `PeerManager::tick()`
  (S2-P12) is the periodic driver.
- **Determinism (V-21 extension)** — prompt-stage2 §6.7 lists peer
  scoring among the Stage 2 network functions that must be
  deterministic: no RNG, no `thread_rng`, no hidden state — same event
  sequence must produce the same score.

## Decision

**We will implement weighted multi-factor peer scoring — a clamped
weighted sum of normalized grading factors (latency, uptime) minus
linear counter penalties (invalid blocks, protocol violations),
evaluated periodically by `PeerManager::tick()`, driving a four-state
ban state machine (NoAction → Warn → TempBan → PersistentBan) with
thresholds tunable in `config.toml` and persistent bans stored in
`network.json`.**

| Aspect | Decision |
|---|---|
| Factors | 4: `latency_ewma_ms`, `invalid_blocks_txs_sent`, `protocol_violations`, `uptime_norm` (responsive fraction) |
| Latency EWMA | `ewma = 0.9 * ewma + 0.1 * sample_ms` (α = 0.1, updated on every message) |
| Latency normalization | `latency_norm = clamp(ms / 100, 0, 1)` — ≥100 ms scores zero on this factor |
| Uptime normalization | `uptime_norm = responsive_secs / observed_secs` since first sighting, clamped `[0, 1]` — «uptime percentage 0..100% → 0..1» from the prompt: a responsive peer sits at 1.0 from its first tick (no new-peer cold start), a flaky peer decays toward 0 |
| Quality normalization | grading part is scaled to `[0, 1]` by its own maximum: `quality = (w_latency·(1−lat_norm) + w_uptime·uptime_norm) / (w_latency + w_uptime)` — required because the grading weights sum to only 0.3, below the 0.4 warn threshold (see Design note) |
| Counter penalties | `w_invalid * (invalid_blocks_txs_sent / INVALID_FULL_WEIGHT) + w_violation * (protocol_violations / VIOLATION_FULL_WEIGHT)` — linear, **no upper clamp** so severe abuse can reach the persistent threshold; initial `INVALID_FULL_WEIGHT = 6`, `VIOLATION_FULL_WEIGHT = 4` (tunable) |
| Weights (initial) | `w_latency = 0.2`, `w_invalid = 0.5`, `w_violation = 0.2`, `w_uptime = 0.1` |
| Ban thresholds (initial) | warn at `score < 0.4`, temp ban at `score < 0.2`, persistent ban at `score < 0.05` |
| Ban durations (initial) | temp ban 1 h, persistent ban 30 d |
| Evaluation cadence | `PeerManager::tick()` every 60 s, for every peer |
| Tuning surface | all weights, thresholds, durations, full-weight constants in `config.toml` (`peer_scoring_*` keys, S2-P17) |
| Ban persistence | `network.json: banned_peers` (schema below); evaluated at load, expired entries dropped |
| Ban key | `node_id` (Ed25519 static identity, ADR-0020/S2-P09 — does not rotate) |

### Scoring formula

```
quality = ( w_latency * (1 − latency_norm) + w_uptime * uptime_norm )
          / (w_latency + w_uptime)                                        # grading part, ∈ [0, 1]

abuse   = w_invalid   * (invalid_blocks_txs_sent / INVALID_FULL_WEIGHT)   # penalties,
        + w_violation * (protocol_violations  / VIOLATION_FULL_WEIGHT)    # unclamped, ≥ 0

score   = clamp01(quality − abuse)                                        # ∈ [0, 1]

initials: w = (0.2, 0.5, 0.2, 0.1), INVALID_FULL_WEIGHT = 6,
          VIOLATION_FULL_WEIGHT = 4
```

**Design note (deviations from the literal formula in
`analytics/prompt-stage2.md` S2-P03/S2-P17 — recorded per the FBP rule):**

1. **Counter terms are subtractive penalties.** The prompt writes them
   additively (`+ w_invalid * invalid_count + …`); taken literally, a
   peer sending *more* invalid blocks would *gain* score — inverting
   «score < threshold → ban» and contradicting ADR-0021 (mismatch rate
   is a *penalty* signal).
2. **The grading part is normalized to [0, 1].** The prompt's grading
   weights sum to `w_latency + w_uptime = 0.3`, which is below the 0.4
   warn threshold — an unnormalized sum would leave *every* clean peer
   permanently in Warn. Dividing by the grading maximum preserves the
   2:1 latency/uptime importance ratio while making the prompt's
   thresholds meaningful on `[0, 1]`.
3. **Counter norms are linear without an upper clamp** (each count /
   full-weight constant), and the constants are chosen so the
   acceptance scenarios of S2-P17 land exactly: with a healthy peer
   (`quality = 1.0`) `invalid_blocks_txs_sent = 10` → `score ≈ 0.167` →
   TempBan; `protocol_violations = 20` → `score = 0.0` → PersistentBan.
   Clamping the norms at 1.0 would cap total abuse at
   `w_invalid + w_violation = 0.7`, making the temp (0.2) and persistent
   (0.05) thresholds unreachable.
4. **Uptime is a responsive fraction, not cumulative-contact-seconds.**
   The prompt's factor wording («uptime percentage 0..100% → 0..1»)
   matches the fraction semantics; the cumulative variant sketched in
   S2-P17 (`uptime_seconds / MAX_UPTIME_SEC`, 30-day cap) would give a
   newly-seen peer ≈ 0 — and a newly-seen *slow* peer would then score
   0 and be banned at its first tick. The fraction gives every
   responsive peer 1.0 from the start and only decays for peers that
   stop responding.

### Milestones (healthy peer: `quality = 1.0`)

| Event count | abuse | score | State |
|---|---|---|---|
| 0 | 0 | 1.000 | NoAction |
| `invalid = 7` | 0.583 | 0.417 | NoAction |
| `invalid = 8` | 0.667 | 0.333 | Warn |
| `invalid = 10` | 0.833 | **0.167** | **TempBan** (S2-P17 scenario) |
| `invalid = 12` | 1.000 | 0.000 | PersistentBan |
| `violations = 16` | 0.800 | 0.200 | Warn (boundary — temp requires `< 0.2`) |
| `violations = 17` | 0.850 | 0.150 | TempBan |
| `violations = 20` | 1.000 | **0.000** | **PersistentBan** (S2-P17 scenario) |
| slow (≥100 ms) + responsive, no violations | 0 | 0.333 | Warn — never banned (false-positive path) |

Graduated progression follows from monotonicity: counters only grow,
score only falls, so a peer crosses NoAction → Warn → TempBan →
PersistentBan in order (unless counters stop growing and quality
recovers). Counters are event-driven: an invalid block increments
`invalid_blocks_txs_sent`; rate-limit exceed, malformed messages and
other protocol misuse increment `protocol_violations` (S2-P16 maps its
violations to the latter).

### Ban state machine

```
  NoAction (score ≥ 0.4)
     │ score < 0.4
     ▼
  Warn — connected, tracing::warn! + reduced broadcast priority
     │ ▲
     │ └── recovery (uptime grows, no new violations): score ≥ 0.4 ─▶ NoAction
     │ score < 0.2
     ▼
  TempBan 1h (in-memory)
     │ └── tick: until < now ─▶ unban (record → historical_bans, audit)
     │ score < 0.05
     ▼
  PersistentBan 30d ─▶ written to network.json, survives restart
```

| Condition | Action | Duration | Persisted |
|---|---|---|---|
| `score ≥ 0.4` | NoAction | — | no |
| `0.2 ≤ score < 0.4` | Warn — `tracing::warn!` + reduced broadcast priority; connection stays up | until score recovers | no |
| `0.05 ≤ score < 0.2` | TempBan — connection dropped, reconnect refused until expiry | 1 h (tunable) | no (in-memory; expiry via `tick()`) |
| `score < 0.05` | PersistentBan — connection dropped, entry written to `network.json` | 30 d (tunable) | **yes** |

Ban expiry: `PeerManager::tick()` checks `ban_until < now` → unban and
move the record to a `historical_bans` audit table (not deleted).
Existing Stage 1 rate-limiter bans (5-min, per-address) remain as-is;
rate-limit *violations* additionally increment `protocol_violations`
through the peer_manager API (S2-P16 integration).

### network.json schema extension

Current Stage 1 schema is `{ "peers": ["ip:port", …] }`. Extension
(backward compatible — a missing key reads as an empty list; existing
files without the key keep working):

```json
{
  "peers": ["127.0.0.1:8081"],
  "banned_peers": [
    {
      "node_id": "hex-blake3-ed25519-pubkey",
      "ban_until": "2026-11-23T12:00:00Z",
      "reason": "persistent_ban: score 0.03 < 0.05 (invalid_blocks=11, protocol_violations=1)",
      "banned_at": "2026-10-24T12:00:00Z"
    }
  ]
}
```

- `ban_until` in the past at load time → entry dropped (expired ban does
  not resurrect).
- `reason` is a free-form string for operator audit; not parsed.
- The `peers` array format is untouched here — richer per-peer records
  (`peer_store` state) are specified in S2-P12.

### False-positive mitigation (ROADMAP3 risk)

The structural guarantee that a legitimate-but-slow peer is never
banned:

- **Latency is a grading factor, not a ban trigger.** Worst case
  (latency ≥ 100 ms permanently) the grading part loses the full
  `w_latency` share — for a fully responsive peer (`uptime_norm = 1`)
  the floor is `(w_uptime · 1) / (w_latency + w_uptime) = 0.1/0.3 ≈
  0.333`. With no violations that is the *entire* score: 0.333 sits in
  the Warn band but comfortably above the temp threshold (0.2). **A
  responsive peer with zero violations is mathematically unbannable,
  whatever its latency.**
- **Uptime starts at 1.0 for responsive peers** (fraction semantics) —
  a brand-new peer is not penalized for being new; the factor only
  decays when the peer actually stops responding.
- **Only counters ban** — `invalid_blocks_txs_sent` (w = 0.5) and
  `protocol_violations` (w = 0.2), both fed by *protocol-level*
  misbehavior (invalid blocks/txs, rate-limit exceed, malformed
  messages), never by mere slowness. Crossing temp/persistent requires
  `abuse ≥ 0.8` / `≥ 0.95` — unreachable without counter growth.
- **Warn precedes any ban.** A peer crossing 0.4 gets a logged warning
  and reduced broadcast priority while still connected — degradation
  (worsening latency, decaying responsiveness, new violations) passes
  through Warn first, and recovery (improved quality, no new
  violations) pulls the score back before any disconnect happens.
- **Tunability** — operators observing false positives adjust weights,
  thresholds and full-weight constants in `config.toml` without
  recompiling (ROADMAP3 mitigation: «tunable thresholds; monitoring of
  false positive rate»).

### Determinism requirements

Per Stage 2 Determinism Checklist (threat V-21 extension; prompt-stage2
§6.7 explicitly includes peer scoring):

1. `compute_score` is a pure function of the four factor values and the
   config — no `SystemTime`, no `Instant`, no RNG, no I/O.
2. Counters and EWMA update only from observed protocol events (message
   received, invalid block rejected, rate violation flagged) — same
   event sequence ⇒ same score on every node replaying it.
3. No `thread_rng` / `OsRng` anywhere in the scoring path.
4. Wall clock appears only at the *edges*: ban expiry comparison
   (`ban_until < now`) in `tick()` and timestamps written to
   `banned_peers`. This is ban **policy**, not consensus state — it does
   not affect block/tx validation, so `cargo test --features
   deterministic_replay` is unaffected; tests inject a simulated clock.
5. Peer iteration order in `tick()` does not influence results — each
   peer's score depends only on its own record.

## Consequences

### Positive

- **Tunable defense** — weights, thresholds and durations live in
  `config.toml`; operators tune sensitivity post-deploy without a
  release cycle (direct ROADMAP3 risk mitigation).
- **False-positive path is structural** — latency/uptime can grade but
  not ban; only protocol-level counters cross ban thresholds (see
  False-positive mitigation above).
- **Persistent bans survive restart** — terminal-state bans are in
  `network.json`, keyed by a non-rotating `node_id` (ADR-0020
  identity); an attacker cannot shed a 30-day ban by rotating IPs.
- **Composes with the protection stack** — rate limiter (ADR-0023)
  feeds `protocol_violations`; Erlay mismatch rate (ADR-0021) surfaces
  as the same penalty channel; connection-pool per-IP limits (S2-P12)
  handle Sybil at connection time while scoring handles it over time.
- **Graduated response** — warn → temp → persistent gives operators a
  visible intermediate state and gives slow-but-honest peers multiple
  recovery chances before any disconnect.

### Negative

- **No universally optimal weights** — 0.2 / 0.5 / 0.2 / 0.1 are
  engineering defaults, not measured optima; mis-tuned weights either
  under-protect or false-positive. Mitigation: tunable config + the
  integration tests (S2-P17 / S2-P22) pin the intended behavior.
- **In-memory counter loss on restart** — `latency_ewma`,
  `invalid_blocks_txs_sent`, `protocol_violations` and the
  responsive/observed uptime accumulators are in-memory; a node restart
  resets scoring history, so a peer that was merely *close* to a ban
  threshold starts fresh. Only the terminal state (PersistentBan) is
  durable. Accepted for Stage 2; durable score persistence would
  piggyback on `network.json` later if needed.
- **Persistent `banned_peers` list can grow** — unbounded without
  expiry pruning; bounded in practice by the 30-day TTL (expired
  entries dropped at load) and by operator cleanup.
- **Full-weight constants are engineering defaults** — `6` invalid /
  `4` violations per full penalty weight encode an assumption about
  attack intensity (chosen to land the S2-P17 acceptance milestones);
  they are unvalidated against real traffic. Mitigation: constants are
  tunable in `config.toml` alongside the weights, and counters are
  cumulative-in-memory, so even a slow-drip attacker (1 invalid block
  per hour) crosses the persistent threshold in ~12 h without a
  restart.

### Neutral

- **Scoring is relay/peer policy, not consensus** — no SCIP /
  activation height needed pre-mainnet-freeze; changes are config-only.
- **Integration points are fixed here, implemented later** — S2-P12
  (PeerStore/PeerManager skeleton, `tick()`), S2-P16 (rate violation →
  `protocol_violations++`), S2-P17 (`scoring.rs` full implementation +
  `config.toml` keys), S2-P22 (`net_peer_scoring.rs` e2e).
- **Stage 1 rate limiter is not replaced** — its 5-min per-address ban
  stays as the fast path; scoring is the slow, reputation-based path on
  top (invariant #17 extension, S2-P23).
- **Audit trail** — unbanned records move to `historical_bans` rather
  than being deleted, so false-positive investigations have data.

## Alternatives Considered

### Alternative 1: Hard ban after single violation

- Pros: trivial to implement and reason about; immediate response to
  any misbehavior; no weights to tune.
- Cons: false positives are guaranteed — one dropped packet, one
  malformed frame during a network glitch, or one invalid block caused
  by the peer's own sync race bans an honest node. ROADMAP3 explicitly
  lists «Peer scoring может ban legitimate peers» as a Stage 2 risk;
  this design makes that risk near-certain instead of mitigating it.
- Why not chosen: false-positive cost (losing honest peers from the
  graph, harming eclipse resistance V-1) outweighs simplicity.

### Alternative 2: No scoring (Stage 1 status quo)

- Pros: zero code, zero tuning, no false positives by construction.
- Cons: threats V-15 (invalid-block DoS), V-17 (Sybil) and V-1
  (Eclipse) stay unmitigated — an attacker repeats invalid blocks or
  floods connections with no accumulated consequence. ROADMAP3 Этап 2
  DoD («Peer scoring: malicious peers banned после N violations») fails.
- Why not chosen: ARCHITECT3 §6 and ROADMAP3 require scoring; doing
  nothing is not a Stage 2 option.

### Alternative 3: Reputation tokens / stake-based reputation

- Pros: Sybil-expensive by construction — fake peers must burn stake;
  economically grounded in the long run.
- Cons: requires an on-chain reputation contract and a stake/ slashing
  model (PoS territory, Stage 7); consensus-coupled (SCIP + activation
  height); heavy machinery for a pre-mainnet network. ARCHITECT3 §15
  anti-goal territory for MVP scope.
- Why not chosen: overengineered for Stage 2; revisit with PoS (Stage 7)
  when stake exists.

### Alternative 4: Sliding-window rate-based scoring only

- Pros: simple counters per fixed window; directly extends the Stage 1
  rate limiter; no EWMA or uptime bookkeeping.
- Cons: catches only *rate* violations — says nothing about invalid
  blocks (V-15) sent slowly, and nothing about connection quality; a
  patient attacker stays under every rate threshold while still being
  harmful. No reputation dimension for eclipse mitigation (V-1).
- Why not chosen: covers one threat class (spam, which ADR-0023 handles
  separately) while leaving V-15/V-1 without defense.

## Related

- ARCHITECT3 §3.6 (network/p2p — peer_store / peer_manager /
  connection_pool split), §6 threats V-1, V-15, V-17, §15 (anti-goals —
  reputation tokens out of scope)
- ARCHITECT.md §11.6 (peer scoring as a separate subsystem — factors +
  tunable ban threshold + persistent bans)
- ROADMAP3 Этап 2 — task «Peer scoring (расширение Stage 1)»; DoD
  «Peer scoring: malicious peers banned после N violations»; risk
  «Peer scoring может ban legitimate peers» + mitigation «tunable
  thresholds; monitoring of false positive rate»
- ADR-0020 (Noise Protocol Framework — `node_id` identity that keys
  bans; P2P transport the peers communicate over)
- ADR-0021 (Erlay — sustained mismatch rate surfaces as a peer quality
  signal into this scoring)
- ADR-0023 (Rate limiting — rate violations increment
  `protocol_violations` here; ban threshold interplay)
- Stage 2 prompts: S2-P04 (ADR-0023, parallel ADR), S2-P12 (peer split +
  scoring skeleton), S2-P16 (rate limiter integration), S2-P17 (full
  `scoring.rs` implementation — `ScoringConfig`, `compute_score`,
  `update_score`, `config.toml` keys), S2-P22 (`net_peer_scoring.rs`
  e2e: malicious banned / false-positive not banned / persistent
  survives restart)
- Stage 2 Determinism Checklist (AGENTS.md) — scoring is pure-function
  of counters; V-21 extension
- docs/security/THREAT_MODEL.md — V-1 / V-15 / V-17 mitigations
  (updated in S2-P23)

## Renumbering note

ROADMAP3 originally planned this ADR as ADR-0018 (third of the planned
ADR-0016..0019 block for Stage 2). Stage 1.5 FBP07 occupied ADR-0016 for
«Receipts Root in Block Header», so all Stage 2 ADRs were renumbered to
ADR-0020..0023 (noise / erlay / peer-scoring / rate-limiting). See the
full renumbering note in ADR-0020; the decision is recorded in the
S2-P01 commit message.
