# ADR-0027: License Change — Apache-2.0 Only (supersedes ADR-0004)

## Status
Accepted (2026-11-01) — supersedes [ADR-0004](0004-license.md)

## Context

ADR-0004 selected a dual **MIT OR Apache-2.0** license for Strangecoin, following the common Rust-ecosystem convention. In practice the repository has always shipped a single `LICENSE` file, and downstream consumers have to reason about two alternative grant texts instead of one.

The project now moves to a single license:

- **Apache-2.0 only** — one text, one SPDX identifier (`Apache-2.0`), one grant to reason about.
- The original motivation for the dual option (Rust-ecosystem familiarity) is outweighed by the value of an **explicit patent grant** for a cryptocurrency whose consensus, cryptography, and wire code may touch patented techniques — the very reason Apache-2.0 was already the "preferred" half of the dual choice in ADR-0004 §Alternatives.

The MIT option is removed entirely; there is no longer a choice for downstream users to make.

## Decision

Strangecoin is licensed under the **Apache License, Version 2.0** only.

1. The root [`LICENSE`](../../LICENSE) file contains the full text of the Apache License 2.0, with the Appendix copyright line:
   `Copyright 2026 Strangecoin Contributors`.
2. The SPDX identifier `Apache-2.0` is declared in the workspace `Cargo.toml` and in every workspace-member `Cargo.toml`.
3. No source file carries a dual MIT/Apache header; no `LICENSE-MIT` / `LICENSE-APACHE` files exist.
4. ADR-0004 is **superseded** (banner added to its Status section); its text is preserved as the historical record of the Stage 0 decision.

## Consequences

### Positive
- **Single grant, single SPDX id**: tooling, license scanners, and corporate approval see exactly one license.
- **Explicit patent grant** from all contributors (Apache-2.0 §3), including patent-litigation termination as a defensive measure (§3 final clause).
- **Contributor terms are unambiguous**: contributions are accepted under Apache-2.0 only (no dual-licensing negotiation).

### Negative
- **Loses the MIT option** some downstream users prefer for maximum simplicity.
- **Diverges from the default Rust-ecosystem dual-license convention**; crates that are MIT-only remain compatible (Apache-2.0 may incorporate MIT-licensed code), but our code can no longer be consumed under MIT terms.

### Neutral
- Existing contributions before this ADR were made under the dual MIT OR Apache-2.0 grant; Apache-2.0 was always one of the two offered terms, so relicensing to Apache-2.0-only does not require additional contributor action for code already under that grant.
- Dependency licensing is unaffected: dependencies may remain MIT/Apache-2.0 (see `CONTRIBUTING.md`).

## Alternatives

### Alternative 1: Keep dual MIT OR Apache-2.0 (ADR-0004 status quo)
- **Pros**: Rust-ecosystem convention; downstream choice.
- **Cons**: Two texts to maintain; license scanners report ambiguity; the MIT half carries no patent grant.
- **Why not chosen**: the project wants one unambiguous license with a patent grant.

### Alternative 2: MIT only
- **Pros**: Shortest, most widely recognized text.
- **Cons**: No explicit patent grant — the deciding factor against MIT in ADR-0004 §Alternative 1.
- **Why not chosen**: patent protection matters more than brevity for this codebase.

### Alternative 3: GPL-3.0 / AGPL-3.0
- **Pros**: Share-alike enforcement for forks.
- **Cons**: Incompatible with the MIT/Apache-2.0 dependency graph; deters commercial and institutional adoption.
- **Why not chosen**: same reasoning as ADR-0004 §Alternative 3.

## Related
- [ADR-0004](0004-license.md) — original dual-license decision (Superseded by this ADR)
- [ADR-0005](0005-tracing-vs-println.md) — references ADR-0004 historically
- Apache License 2.0: https://www.apache.org/licenses/LICENSE-2.0
- SPDX License List: https://spdx.org/licenses/Apache-2.0
