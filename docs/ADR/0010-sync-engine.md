# ADR-0010: SyncEngine (inbox pattern)

## Status

Accepted

## Context

Before this ADR, the network layer called the blockchain directly from multiple
concurrent contexts. The coupling is a call-graph/ownership cycle rather than a
module cycle: connection handler threads and the sync task all hold
`Arc<BlockchainFacade>` and invoke state-changing methods inline.

Direct call sites at the time of writing:

| Site | Call | Kind |
|------|------|------|
| `src/lib.rs` inbound `UPDATE_BLOCKCHAIN` handler | `adopt_candidate` / `apply_tx` / `save_state` | write |
| `src/lib.rs` `sync_blockchain` legacy `GET_BLOCKCHAIN` fallback | `adopt_candidate` / `apply_tx` / `save_state` | write |
| `src/network/sync.rs::sync_headers_first` (phase 4) | `adopt_candidate` | write |
| `src/lib.rs` inbound `GET_HEADERS` / `GET_BLOCKS` / `GET_BLOCKCHAIN` handlers | `headers_from_height` / `blocks_by_hashes` / `to_wire_json` | read |
| GUI `sync_rx` drain | `adopt_wire` | write (not network) |

The write sites race. Each individual facade call is serialized by the inner
`RwLock`, but **validate → apply → announce is not one atomic pipeline**: a
headers-first download, a gossip `UPDATE_BLOCKCHAIN` from a second peer, and a
legacy fallback can interleave between lock acquisitions — each reads the tip,
validates against it, and applies, so two peers pushing the same block or two
different forks can produce duplicated announcements, torn snapshots to the GUI,
and work computed against a stale tip. There is no single point that decides
processing order, so arrival order across threads is the de-facto ordering.

Motivating forces:

- ARCHITECT3 §3.6 / ROADMAP3 Stage 1 require: *NetworkService does not call
  blockchain directly — it only puts incoming blocks into `SyncEngine.inbox`
  (mpsc); SyncEngine is the single component that performs validate → apply →
  announce*.
- The DoD criterion for Stage 1 is "Sync engine breaks the cycle
  network↔blockchain".
- Determinism: with a single consumer, processing order is explicit and
  testable, instead of emergent from thread scheduling.
- Backpressure: unbounded direct calls mean a spamming peer costs whatever the
  validation path costs, paid on the handler thread, with no place to drop or
  ban.

Prerequisites already in place: tokio runtime (ADR-0007, S1-P10), `EventBus`
(ADR-0009, S1-P09), `BlockchainFacade` as the sole public API of the blockchain
(S1-P13), headers-first sync producing candidates (S1-P16).

## Decision

We will introduce a **SyncEngine** that owns the incoming-data inbox and is the
only component performing validate → apply → announce.

Key design choices:

- **Inbox**: `tokio::sync::mpsc`, **bounded**, split into two channels by
  priority — a headers channel and a blocks/tx channel. Network handler threads
  (std) enqueue with `try_send`; the engine task drains with `recv().await`,
  biased toward headers so HEADERS of a peer are processed before BLOCKS of the
  same peer (and before any pending blocks from any peer).
- **`Incoming` enum**: `NewBlock`, `NewHeaders`, `NewTx`, and `CandidateChain`
  (the adopted-shape payload: full candidate chain + balances + mempool +
  difficulty — used both by the legacy `UPDATE_BLOCKCHAIN` gossip path and by
  the headers-first download result). Each variant carries
  `from: Option<SocketAddr>` for attribution (rate limiting / bans).
- **Single consumer, strictly sequential**: one `SyncEngine::run` task processes
  one message to completion (validate via facade/block_executor, apply, announce)
  before taking the next. Determinism is a property of the architecture, not of
  lock timing.
- **Announce**: after a successful apply the engine publishes
  `StatePersisted`/`BlockApplied` (and `BlockReorged` when the tip changed
  through a fork) on the `EventBus` and sends the GUI snapshot over the existing
  `sync_tx` channel. Gossip push to peers stays pull-driven in Stage 1 (the sync
  loop pushes our chain when it dials a peer); event-driven broadcast gossip is
  Stage 2 scope.
- **Read-serving stays on the facade**: `GET_HEADERS`, `GET_BLOCKS` and
  `GET_BLOCKCHAIN` handlers answer through read-only `BlockchainFacade` calls
  (`headers_from_height`, `blocks_by_hashes`, `to_wire_json`). Reads under the
  `RwLock` do not participate in the validate → apply → announce pipeline and
  therefore cannot race it; routing them through the engine would add a
  std↔tokio response-channel bridge to every request for zero correctness gain.
  The audit rule is: *network may use facade-API reads and the inbox; network
  must not call write paths (`adopt_candidate` / `adopt_wire` / `apply_tx` /
  `save_state`) or touch blockchain internals.*
- **Backpressure**: bounded channels; on a full channel the producer drops the
  message. Duplicates are dropped before they cost validation work (the engine
  keeps a bounded seen-set of recent block hashes/txids), and peers that keep
  pushing into a full inbox are banned through the existing `RateLimiter`.
- **Legacy text protocol kept**: `UPDATE_BLOCKCHAIN` / `GET_BLOCKCHAIN` remain
  on the wire for compatibility with pre-P16 peers, but `UPDATE_BLOCKCHAIN` now
  only parses and enqueues `CandidateChain` — adoption happens in the engine.

## Consequences

### Positive

- One ordering point: races between gossip, headers-first sync, and legacy
  fallback disappear because all three funnel into one sequentially-drained
  inbox. The race scenario from S1-P18 (two peers pushing the same block and
  different forks concurrently) becomes a queue-order question with a
  deterministic final state.
- Single audit point for state transitions: `SyncEngine` is the only caller of
  the write API, so invariant checks, event emission, and telemetry live in one
  place instead of four.
- Backpressure is explicit: bounded channels + dedupe + ban turn inbox spam
  into a cheap drop instead of validation work on handler threads.
- HEADERS priority makes fork discovery latency independent of body-download
  backlog.
- The network layer stops importing blockchain state-transition types; the
  cycle network↔blockchain is broken at the ownership level.

### Negative

- Write latency gains one queue hop (bounded by channel capacity and engine
  throughput). Acceptable: adoption already does full `validate_chain` work.
- One more moving part (engine task) and a std→tokio bridge (`try_send` from
  handler threads) to keep correct under shutdown — the engine must drain until
  the channels close or shutdown flips.
- If the engine task dies, writes stall silently while reads keep working;
  mitigated by logging on task exit and the shutdown watchdog.
- `Node` construction in tests needs inbox senders wired, increasing fixture
  setup slightly.

### Neutral

- The GUI `sync_rx → adopt_wire` snapshot path is **not** migrated in this
  ADR: it is not part of the network layer and ADR-0009 already deferred full
  GUI migration. It remains a known fourth adoption site, recorded here as
  residual coupling to be closed when the GUI moves to EventBus-driven updates.
- Mining-thread calls (`apply_tx`, `mine_block`) are out of scope: mining is a
  producer of blocks, not the network ingest path.
- `blockchain → network::protocol` size-constant imports are a reverse-direction
  layering nit; they do not participate in the state-transition cycle and are
  left for a later cleanup.

## Alternatives

### Alternative 1: Direct calls with finer-grained locks

- Pros: no new component; minimal diff.
- Cons: does not establish processing order — `RwLock` serializes individual
  calls, not validate → apply → announce pipelines; races persist; no place for
  backpressure or dedupe; violates the DoD criterion outright.
- Why not chosen: the problem is ordering, not atomicity of single calls.

### Alternative 2: One worker per peer

- Pros: natural per-peer flow control.
- Cons: no global ordering — two peers' blocks still interleave mid-pipeline,
  which is exactly the race being fixed; inconsistent event counts across
  identical inputs.
- Why not chosen: determinism requires a single consumer.

### Alternative 3: Strict all-traffic inbox (reads included, oneshot responses)

- Pros: absolute single access point to the facade.
- Cons: every `GET_HEADERS`/`GET_BLOCKS` response becomes an inbox round-trip
  with a response channel bridged from std handler threads into the async
  engine; adds latency and failure modes to reads that are already correct
  under `RwLock` and outside the validate → apply → announce pipeline.
- Why not chosen: the cycle to break is the state-transition cycle (ARCHITECT3
  §3.6 speaks of "incoming blocks"); reads cannot race the pipeline. The audit
  rule keeps them on facade-API where they are cheap and safe.

### Alternative 4: SyncEngine owns the TCP download loop

- Pros: even fewer components touch sockets.
- Cons: conflates transport (timeouts, framing, retries) with state transition;
  the headers-first loop is already layered and tested in `network/sync.rs`
  (HeaderCache → plan → download); moving TCP into the engine makes the engine
  hard to test without sockets.
- Why not chosen: the download loop stays network-side and *emits* `Incoming`
  messages; adoption is what moves.

### Alternative 5: crossbeam channel instead of tokio mpsc

- Pros: symmetric with the EventBus (ADR-0009); no async needed for producers.
- Cons: the consumer must be an async task (ADR-0007: tokio runtime is in; a
  crossbeam consumer would need a blocking bridge thread, reintroducing a
  second thread to order against); `tokio::sync::mpsc::Sender` already supports
  `try_send` from non-async threads, so producers stay simple.
- Why not chosen: tokio mpsc gives async `recv().await` for the engine and
  sync `try_send` for handler threads with one channel type.

## Related

- ARCHITECT3 §3.6, §10.2 — SyncEngine specification
- ROADMAP3 Stage 1 — "Sync engine breaks the cycle network↔blockchain" DoD
- ADR-0007 (tokio on Stage 1) — runtime the engine runs on
- ADR-0009 (EventBus) — announce channel; `subscribe_async` bridge noted for
  the engine
- S1-P13 `BlockchainFacade` — the write API the engine exclusively owns
- S1-P16 headers-first sync — download loop that emits `Incoming::CandidateChain`
- S1-P18 implementation prompt this ADR gates
