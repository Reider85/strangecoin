# Strangecoin fuzzing (S1-P19)

First fuzz target of the security track (ROADMAP3: «Stage 1+: Fuzzing
(cargo-fuzz)»): the canonical deserializers in
`strangecoin-core/src/serialize.rs` on arbitrary bytes. Contract: every input
either decodes or returns a typed error — a panic is a crash.

## Target

- `fuzz_targets/canonical_decode.rs` — feeds raw bytes to
  `deserialize_header` / `deserialize_block` / `deserialize_transaction` /
  `deserialize_transaction_signed`.

## Running on a capable host (Linux, macOS, MSVC)

```bash
cargo install cargo-fuzz
cargo fuzz run canonical_decode -- -max_total_time=600
```

Requires a nightly toolchain; libFuzzer + ASan provide coverage guidance and
crash detection. Corpus: `fuzz/corpus/canonical_decode/`; artifacts (crashes):
`fuzz/artifacts/`.

## Result on the S1-P19 dev machine (Windows, 2026-10-04)

The coverage-guided run is **not possible on this machine** — the reason
(«причина фиксации» per the S1-P19 checklist):

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

**Result (2026-10-04):** the fallback paid off immediately — within seconds it
found an out-of-bounds panic in the block decoder on a mutated real block
(`read_u32_be`/`read_u64_be` indexed past the end of truncated frames, e.g. a
36-byte transaction slice at `deserialize_transaction_signed`'s `sig_len`
read; the block loop also relied on a stale `remaining` check). Fixed in
`crates/strangecoin-core/src/serialize.rs`: the integer readers are now
bounds-checked (`Result`), every decoder call site propagates errors, and the
per-transaction length check uses the live offset. Post-fix soak runs: 15 s
smoke clean; **10-minute run: 28,744,131 inputs, 0 panics, exit 0**.

On any host with a supported toolchain, prefer the cargo-fuzz command above
and re-record the result here; S1-P21 folds the outcome into
`docs/security/THREAT_MODEL.md` Monitoring.
