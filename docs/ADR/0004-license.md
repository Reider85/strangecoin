# ADR-0004: License Selection — Dual MIT OR Apache-2.0

## Status
Accepted

## Context
Strangecoin is an open-source cryptocurrency project written in Rust. We need to choose a license that:
- Allows maximum adoption and integration with the Rust ecosystem
- Is compatible with all our dependencies (rusty-leveldb, ed25519-dalek, sha2, pbkdf2, aes-gcm, eframe, etc.)
- Provides clear patent grants for contributors and users
- Aligns with community conventions for Rust projects

The main options considered were:
- **MIT** — Simple, permissive, widely understood. No explicit patent grant.
- **Apache-2.0** — Permissive with explicit patent grant, used by major Rust projects (tokio, serde, reqwest, etc.).
- **GPL-3.0** — Copyleft, restricts commercial use, incompatible with many Rust ecosystem crates.
- **Dual MIT OR Apache-2.0** — Used by the Rust standard library and most core crates. Allows downstream users to choose whichever license suits them.

## Decision
We will license Strangecoin under **dual MIT OR Apache-2.0**.

All source files will carry the standard dual-license header:
```text
// Copyright (c) 2026 Strangecoin Contributors
// Licensed under the Apache License, Version 2.0 or the MIT license, at your option.
// See LICENSE-MIT and LICENSE-APACHE for details.
```

The repository contains a single `LICENSE` file with the full text of both licenses.

## Consequences

### Positive
- **Ecosystem compatibility**: Matches the licensing of virtually all Rust dependencies. No license conflicts when adding new crates.
- **Downstream flexibility**: Users can choose MIT (simpler) or Apache-2.0 (patent grant) based on their needs.
- **Patent protection**: Apache-2.0 provides explicit patent grants from contributors, reducing IP risk.
- **Community standard**: Familiar to Rust developers; no friction for contributors or corporate users.
- **No copyleft concerns**: Both licenses are permissive; no viral clauses that would deter adoption.

### Negative
- **No copyleft**: Cannot enforce share-alike; companies can build proprietary forks without contributing back.
- **Two license texts**: Slightly more complex than single-license projects (mitigated by standard dual-license boilerplate).

### Neutral
- Contributors must agree to license their contributions under both licenses (standard DCO / CLA approach).

## Alternatives

### Alternative 1: MIT Only
- **Pros**: Simpler, single license file.
- **Cons**: No explicit patent grant. Some corporate legal teams prefer Apache-2.0 for patent clarity.
- **Why not chosen**: Patent grant is valuable for a cryptocurrency project where cryptographic implementations may touch patented techniques.

### Alternative 2: Apache-2.0 Only
- **Pros**: Patent grant included, single license.
- **Cons**: Some minimalists prefer MIT; Apache header is more verbose.
- **Why not chosen**: Dual licensing is the established Rust convention (std, tokio, serde, clap, etc.). No downside to offering both.

### Alternative 3: GPL-3.0 / AGPL-3.0
- **Pros**: Ensures derivatives remain open source.
- **Cons**: Incompatible with most Rust ecosystem crates (MIT/Apache). Would prevent using standard libraries. Deters commercial adoption.
- **Why not chosen**: Fundamentally incompatible with the Rust ecosystem and our dependency graph.

## Related
- ADR-0005: Tracing vs println! (logging infrastructure decision)
- Rust Licensing Guidelines: https://rust-lang.github.io/rfcs/2381-opt-in-builtin-trait-impls.html
- SPDX License List: https://spdx.org/licenses/