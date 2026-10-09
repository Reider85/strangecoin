# Strangecoin fuzzing (S1-P19)

First fuzz target of the security track (ROADMAP3: «Stage 1+: Fuzzing
(cargo-fuzz)»): the canonical deserializers in
`strangecoin-core/src/serialize.rs` on arbitrary bytes. Contract: every input
either decodes or returns a typed error — a panic is a crash.

## Target

- `fuzz_targets/canonical_decode.rs` — feeds raw bytes to
  `deserialize_header` / `deserialize_block` / `deserialize_transaction` /
  `deserialize_transaction_signed`.

## Running on CI (canonical, BUG-S0-023 / S1.5-P07)

The coverage-guided run lives in `.github/workflows/ci.yml`, job
`fuzz-canonical-decode`:

- triggers on **every push and pull request** (same `on:` as the rest of CI);
- `ubuntu-latest`, Rust **nightly** (libFuzzer + ASan require it),
  `cargo-fuzz` pinned to **0.13.2**;
- `cargo fuzz run canonical_decode -- -max_total_time=600 -rss_limit_mb=2560`
  — the 10-minute run required by the S1-P19 checklist;
- a crash **fails the job**; `fuzz/artifacts/` (repro inputs) is uploaded as a
  workflow artifact (30-day retention) on failure.

The first green/red result of this job is the authoritative coverage-guided
evidence; record it in the results section below when it lands.

## Running on a capable host (Linux, macOS, MSVC)

```bash
cargo install cargo-fuzz
cargo fuzz run canonical_decode -- -max_total_time=600
```

Requires a nightly toolchain; libFuzzer + ASan provide coverage guidance and
crash detection. Corpus: `fuzz/corpus/canonical_decode/`; artifacts (crashes):
`fuzz/artifacts/`.

## Why the S1-P19 dev machine (Windows) could not run cargo-fuzz

The coverage-guided run was **not possible on the S1-P19 dev machine** — the
reason («причина фиксации» per the S1-P19 checklist):

1. **ASan unsupported on the only linkable Rust target.** The host links via
   `rust-lld` (`x86_64-pc-windows-gnu`); rustc nightly rejects
   `-Zsanitizer=address` on that target: *«address sanitizer is not supported
   for this target»*. ASan on Windows requires `x86_64-pc-windows-msvc`.
2. **No MSVC toolchain.** Visual Studio / Build Tools (`link.exe`) are not
   installed, so the msvc target cannot build or link; ASan there is
   therefore unreachable too.
3. **libFuzzer's C++ runtime needs clang or MSVC.** `libfuzzer-sys` vendors
   LLVM's `FuzzerExtFunctionsWindows.cpp`, which uses `__pragma(comment
   (linker, "/alternatename:..."))` — MSVC-only — or clang's
   `__builtin_function_start`; GNU g++ cannot compile it by design. The
   machine has no clang/LLVM (MSYS2 `clang64` is empty), and `cargo install
   cargo-fuzz` additionally fails in MinGW `ld` because of the non-ASCII user
   profile path.

**Fallback executed instead** (same decoder contract, no coverage guidance):
`cargo run --example canonical_decode_soak` — deterministic (seeded xorshift)
random-bytes + mutated-real-block soak against all four decoders.

## Soak results (fallback, no coverage guidance)

| Date | Command | Inputs | Panics | Note |
|------|---------|--------|--------|------|
| 2026-10-04 | `cargo run --example canonical_decode_soak` (10 s smoke) | 474,380 | 0 | S1-P19; found and fixed the OOB panic below |
| 2026-10-09 | `SOAK_SECONDS=600 cargo run --example canonical_decode_soak` | 25,947,107 | 0 | BUG-S0-023: the S1-P19 checklist's 10-minute run |

An earlier revision of this file claimed a «10-minute run: 28,744,131 inputs»
in the S1-P19 commit, but no such run appeared in the S1-P22 verification log
(`docs/stage1/STAGE1_SUMMARY.md` §3 recorded only the 10 s smoke). The
measured 2026-10-09 run above is the authoritative fallback evidence
(BUG-S0-023); coverage-guided evidence comes from the CI job.

**The fallback paid off immediately on 2026-10-04** — within seconds it found
an out-of-bounds panic in the block decoder on a mutated real block
(`read_u32_be`/`read_u64_be` indexed past the end of truncated frames, e.g. a
36-byte transaction slice at `deserialize_transaction_signed`'s `sig_len`
read; the block loop also relied on a stale `remaining` check). Fixed in
`crates/strangecoin-core/src/serialize.rs`: the integer readers are now
bounds-checked (`Result`), every decoder call site propagates errors, and the
per-transaction length check uses the live offset.

On any host with a supported toolchain, prefer the cargo-fuzz command above
and re-record the result here; S1-P21 folds the outcome into
`docs/security/THREAT_MODEL.md` Monitoring.
