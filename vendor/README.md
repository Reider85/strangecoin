# vendor/

## minisketch-rs 0.1.9 (vendored)

Vendored copy of [`minisketch-rs` 0.1.9](https://crates.io/crates/minisketch-rs)
(Apache-2.0, Evgenii P.), pulled from the crates.io registry on 2026-10-30 and
used through `[patch.crates-io]` in the root `Cargo.toml` by
`crates/strangecoin-net` (optional dependency behind the default-off
`minisketch` feature — SCIP-gated full Erlay PinSketch implementation,
`docs/ADR/0021-erlay-integration.md` fallback-dependency escape hatch).

### Why (S3-P04 deviation, 2026-10-30)

Adding the `rocksdb =0.25.0` storage backend (Stage 3) surfaced a hard
dependency-graph conflict:

- `minisketch-rs 0.1.9` (latest upstream release) → build-dep
  `bindgen 0.49` → `clang-sys 0.28`
- `librocksdb-sys 0.19.0` (from `rocksdb 0.25.0`) → build-dep
  `bindgen 0.72` → `clang-sys 1.9.1`

Both `clang-sys` packages declare `links = "clang"`, and cargo admits only
one `links` provider in a dependency graph — the registry crate can never
coexist with rocksdb in one lockfile. Upstream 0.1.9 is the newest release,
so there is no registry-side fix to wait for.

### What differs from upstream (exactly two files)

| File | Diff vs upstream 0.1.9 |
|------|------------------------|
| `Cargo.toml` | build-dep `bindgen` `"0.49"` → `"=0.72.1"` (the same bindgen line librocksdb-sys uses, so `clang-sys` unifies on 1.9.1) + provenance comment |
| `build.rs` | `whitelist_type`/`whitelist_function` → `allowlist_type`/`allowlist_function` (bindgen renamed these between 0.49 and 0.72) + comment |

Everything else — `src/`, the bundled `minisketch/` C++ library, examples —
is byte-identical to upstream.

### Build requirements (only when the `minisketch` feature is ON)

- C++11 compiler (cc-rs; on Windows the AGENTS.md msys64 PATH preamble)
- libclang for bindgen (Windows/msys64: `pacman -S mingw-w64-x86_64-clang-libs`
  provides `mingw64/bin/libclang.dll`; Linux: distro `libclang` package)

The feature is default-OFF and nothing in the tree enables it: the active
Erlay implementation is the pure-Rust capacity-bounded stub in
`crates/strangecoin-net/src/gossip/erlay/mincut.rs`.

### Not a workspace member

The `[patch.crates-io]` entry deliberately replaces the registry crate
*in the dependency graph only*: `cargo <cmd> --workspace` never builds this
directory while the feature is off (an earlier `exclude =` + direct path-dep
approach was rejected — cargo auto-adds in-tree path dependencies as
workspace members regardless of `exclude`).
