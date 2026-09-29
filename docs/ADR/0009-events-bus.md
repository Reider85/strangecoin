# ADR-0009: EventBus (crossbeam-channel)

## Status

Accepted

## Context

Strangecoin's notification system is fragmented across multiple `std::sync::mpsc` channels:

1. **Mining task channel** (`mpsc::Sender<MiningTask>`): one-shot task dispatch from GUI to mining thread
2. **Sync notification channel** (`mpsc::Sender<Blockchain>`): carries full `Blockchain` clones from P2P/mining threads to GUI
3. **Progress/status channels** (`mpsc::Sender<String>`): mining progress and status messages

Problems with this approach:

- **No multi-subscriber broadcast**: `std::sync::mpsc` is single-consumer. Adding a second subscriber (e.g., metrics, test assertions) requires duplicating channels or adding another `mpsc::Sender` plumbing layer.
- **GUI polls via `try_recv()` every frame** (~10ms): the GUI acquires a write lock on the blockchain inside `update()` to process sync notifications, creating contention with mining and P2P threads.
- **No deterministic test synchronization**: integration tests use `std::thread::sleep(150-250ms)` to wait for async operations (P2P sync, mining). This is fragile and slows the test suite.
- **No event type safety**: sync notifications carry a full `Blockchain` clone rather than structured events describing what changed. Consumers cannot distinguish a block application from a reorg without comparing chain lengths.
- **No metrics foundation**: there is no structured event stream that Prometheus/Grafana exporters could subscribe to.

The goal is a single event bus that all subsystems publish to and any number of subscribers can consume from, enabling clean separation between producers and consumers.

## Decision

We will introduce an `EventBus` backed by `crossbeam-channel` unbounded channels.

Key design choices:

- **Channel type**: `crossbeam_channel::unbounded()` — non-blocking `send()`, backpressure-free (slow subscribers are dropped). This matches ARCHITECT3 §11: "publish is non-blocking: try_send + drop slow subscriber".
- **Multi-subscriber**: each `subscribe()` call clones the internal `Sender` into a `Vec<Sender<NodeEvent>>` and returns a new `Receiver`. On `send()`, all receivers get a copy of the event.
- **Slow subscriber handling**: `try_send` on each subscriber; if it fails (disconnected), remove from the list. No blocking, no buffering.
- **Event types**: `NodeEvent` enum covers all Stage 1 events: `BlockApplied`, `BlockReorged`, `TxAccepted`, `TxRejected`, `MiningStarted`, `MiningFinished`, `PeerScoreChanged`, `StatePersisted`. Additional events (e.g., `ConsensusUpgraded`) will be added in later stages.
- **Ownership**: EventBus lives in the monolith (`src/events.rs`), not in `strangecoin-core`. Core is pure logic with 0 I/O; event publishing is a side-effect of the node runtime.
- **No tokio dependency**: crossbeam works with std threads. When tokio is introduced in S1-P10, the EventBus remains crossbeam-based (tokio::broadcast was considered but rejected — see Alternatives).

## Consequences

### Positive

- Deterministic integration tests: `wait_for_event(receiver, predicate, timeout)` replaces `sleep` with precise condition waiting
- Clean subscriber API: any component calls `bus.subscribe()` and receives a `Receiver<NodeEvent>` — no channel plumbing per-consumer
- Structured event data: consumers receive typed events (`BlockApplied { height, hash }`) instead of full `Blockchain` clones
- Foundation for metrics (Stage 2+): Prometheus exporter subscribes to the same event stream
- Non-blocking publish: mining/P2P threads never stall waiting for GUI or slow consumers

### Negative

- One new dependency (`crossbeam-channel`)
- `std::sync::mpsc` channels for mining task dispatch and progress/status remain (they carry different data types and are not broadcast patterns) — partial migration until all uses are audited
- Crossbeam-channel is not `async`-native; when tokio is introduced (S1-P10), async subscribers will need a bridge (`tokio::sync::mpsc` wrapping or `spawn_blocking` for polling)

### Neutral

- GUI continues to use `sync_rx` for full-chain sync notifications during this prompt. Full GUI migration to EventBus subscription is deferred (the GUI needs chain data, not just events).
- `EventBus` is `Arc`-wrapped and shared across threads. Thread safety is provided by `crossbeam_channel::Sender` being `Send + Sync`.

## Alternatives

### Alternative 1: flume

- Pros: API similar to crossbeam-channel, supports both sync and async (`flume::r#async`), lighter dependency tree
- Cons: smaller ecosystem and community adoption than crossbeam; `flume::r#async` adds tokio coupling we want to avoid until S1-P10
- Why not chosen: crossbeam-channel is the de-facto standard for multi-producer multi-consumer channels in Rust; better battle-tested in concurrent systems (used by rayon, crossbeam-utils, etc.)

### Alternative 2: tokio::broadcast

- Pros: native async, built into tokio (which we will add in S1-P10), automatic lagged-consumer handling
- Cons: requires tokio runtime to be initialized (S1-P10 prerequisite); `broadcast::Sender::send()` blocks if all receivers are lagged beyond capacity; adds tokio coupling before we're ready
- Why not chosen: EventBus must be available before tokio introduction (S1-P09 precedes S1-P10). After S1-P10, the EventBus remains crossbeam-based because: (a) existing threads (mining, P2P) are sync and would need bridges anyway, (b) crossbeam is simpler for the current producer/consumer patterns, (c) tokio::broadcast's lagged-consumer behavior is less predictable than drop-slow-subscriber for our use case.

### Alternative 3: Keep std::sync::mpsc with fan-out

- Pros: no new dependency, already in use
- Cons: manual fan-out (iterate over Vec<Sender>), no built-in slow-subscriber handling, no type safety for events, still requires full Blockchain clones for sync
- Why not chosen: the whole point of this ADR is to replace the fragmented mpsc approach with a structured event bus.

## Related

- ARCHITECT3 §11: EventBus specification
- ADR-0007 (tokio on Stage 1): future ADR, EventBus remains crossbeam after tokio introduction
- S1-P10: tokio introduction (EventBus unchanged)
- S1-P18: SyncEngine will publish events through this bus
- `src/lib.rs` lines 142-171: Node/WalletApp structs to be modified
- `tests/network.rs`: sleep-based synchronization to be replaced with event-wait
