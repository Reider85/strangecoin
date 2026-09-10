# ADR-0005: Structured Logging — tracing vs println!

## Status
Accepted

## Context
The current codebase (v0.8.6) uses `println!` macros in approximately 30 locations across `src/main.rs` and `src/wallet.rs` for:
- Block mining progress and results
- Peer connection/disconnection events
- Transaction validation outcomes
- Wallet operations (creation, signing, loading)
- Error reporting and panic messages
- Debugging output

Problems with `println!`:
1. **No log levels** — Everything is printed at the same level. Cannot filter debug vs info vs error.
2. **No structured fields** — Messages are free-form strings. Cannot programmatically extract fields (block height, tx count, peer address, etc.).
3. **No filtering** — Cannot enable/disable logging per module or crate.
4. **No integration** — Cannot hook into metrics systems (Prometheus, OpenTelemetry) or log aggregators (Loki, ELK).
5. **Performance** — `println!` locks stdout on every call; no async/non-blocking option.
6. **Test interference** — Test output mixes with application logs; no way to capture logs for assertions.

The Rust ecosystem has converged on the `tracing` crate for structured, contextual logging:
- `tracing` — Instrumentation API with spans, events, structured fields
- `tracing-subscriber` — Composable subscribers (fmt, env-filter, json, OpenTelemetry)
- Zero-cost when disabled; efficient when enabled
- First-class support in async runtimes (tokio, async-std)

## Decision
We will migrate all `println!`/`eprintln!` calls in `src/main.rs` and `src/wallet.rs` to `tracing` macros:
- `tracing::error!` — Errors, panics, unrecoverable failures
- `tracing::warn!` — Suspicious situations, deprecated behavior, recoverable issues
- `tracing::info!` — Important lifecycle events (block applied, peer connected, mining started/finished)
- `tracing::debug!` — High-volume diagnostic detail (lock waits, retries, protocol messages)
- `tracing::trace!` — Very high volume (individual loop iterations, frame-level tracing)

Subscriber initialization in `main()`:
```rust
tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .with_target(false)
    .init();
```

Default level: `info`. Override via `RUST_LOG=strangecoin=debug` or `RUST_LOG=trace`.

Structured fields where applicable:
```rust
info!(height = block.index, txs = block.transactions.len(), hash = %hex::encode(block.hash), "block applied");
warn!(peer = %addr, reason = "invalid block", "peer sent invalid block");
```

## Consequences

### Positive
- **Structured fields**: Machine-parseable logs; enables log-based metrics and alerting.
- **Levels & filtering**: `RUST_LOG` controls verbosity per module; production runs quiet, debug runs verbose.
- **Ecosystem integration**: Works with `tracing-opentelemetry`, `tracing-loki`, `tracing-appender`, etc.
- **Test support**: `tracing-test` crate allows capturing logs in tests; no more `sleep` hacks.
- **Performance**: Lazy evaluation via `tracing::enabled!`; spans avoid allocation when disabled.
- **Context propagation**: Spans carry context across async boundaries (ready for Stage 1+ tokio).

### Negative
- **Dependency cost**: Adds `tracing` + `tracing-subscriber` (~200KB compiled).
- **Migration effort**: ~30 `println!` calls to convert; must preserve semantic meaning.
- **Learning curve**: Team must learn span/event model, field syntax.

### Neutral
- `eframe/egui` UI text (user-facing messages) stays as-is — not logging.
- No change to error handling; `StrangecoinError` still returned via `Result`.

## Alternatives

### Alternative 1: `log` + `env_logger`
- **Pros**: Simpler API; lighter weight; familiar to pre-2020 Rust developers.
- **Cons**: No structured fields (key-value pairs); no spans; no context propagation; limited filtering granularity.
- **Why not chosen**: Structured fields are required for log-based metrics and automated log analysis (STRIDE mitigation). `log` crate cannot express `info!(height = 123, txs = 4)`.

### Alternative 2: `slog`
- **Pros**: Structured, modular, async-capable.
- **Cons**: More complex API; smaller ecosystem; less common in modern Rust; `tracing` has become the de facto standard.
- **Why not chosen**: `tracing` has better async/span support, wider adoption, and first-class OpenTelemetry integration.

### Alternative 3: Keep `println!` + add log levels manually
- **Pros**: No new dependencies.
- **Cons**: Reinventing poorly; no structured fields; no ecosystem tooling; technical debt.
- **Why not chosen**: Violates ARCHITECT3.md §15 ("logging through tracing, not println!"). Does not scale.

## Related
- ADR-0004: Dual MIT/Apache-2.0 license
- ARCHITECT3.md §15: "logging — through tracing (not println!)"
- tracing crate: https://docs.rs/tracing/
- tracing-subscriber: https://docs.rs/tracing-subscriber/
- OpenTelemetry integration: https://github.com/open-telemetry/opentelemetry-rust