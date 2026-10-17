# Strangecoin fuzzing (S1-P19, extended S1.5-P16)

Coverage-guided fuzzing (cargo-fuzz / libFuzzer) over the two attack
surfaces the project has shipped so far: the canonical deserializers
(S1-P19) and the Stage 1.5 VM host surface (S1.5-P16, threat V-21). Every
target follows one contract: **a panic is a crash; a typed error or a
deterministic value is a pass.**

## Targets

| Target | Surface | Contract |
|--------|---------|----------|
| `canonical_decode` | `deserialize_{header,block,transaction,transaction_signed}` on arbitrary bytes (S1-P19) | decode or typed error; no panic |
| `host_functions` | all 13 `sc.*` host functions called from generated WAT with fuzzer-chosen (in-bounds / OOB / negative) pointers + the precompile router `dispatch_precompile` on the same payload (S1.5-P16) | no panic; same input ⇒ same `ExecutionResult` / same error |
| `gas_metering` | raw bytes / mutated valid WAT / bounded-loop WAT / infinite-loop WAT under a random gas budget (S1.5-P16, threat V-21) | terminates in `Ok` (with `gas_used ≤ gas_limit`) or typed error; deterministic; infinite loops halt via `OutOfFuel` |
| `storage_trie` | random write/delete/commit sequences through `ContractStorageOverlay` + `contract_storage_root` (S1.5-P16, invariant #4/#19 extension) | same op sequence ⇒ same root; undo/redo preserves the root |
| `differential_wasmi_vs_wasmtime` | generated bounded import-free WAT executed in wasmi (pinned `=2.0.0`, our consensus runtime) and wasmtime `=49.0.3` (reference engine) (S1.5-P16) | same outcome class and return value in both engines; a success/failure split is a real bug |

### Differential methodology

Fuel *amounts* are deliberately not compared: wasmi charges 1 per operator,
wasmtime uses a weighted cost table, so `gas_used` is engine-local. The
comparison basis is the outcome class — `Success(i32)` with the same value
or `Failed` on both sides. Generated programs are bounded (loops ≤ 1000
iterations) so neither engine should exhaust the shared 10M budget; the
failure classes exercised are division/remainder by zero, `unreachable`,
invalid modules, and missing/mistyped `main` exports.

Engines are built once per process (`OnceLock`) and modules per iteration —
engine construction (wasmtime Cranelift config) dominates otherwise.

## Running on CI (BUG-S0-023 / S1.5-P07 / S1.5-P16)

The coverage-guided runs live in `.github/workflows/ci.yml`, job `fuzz`
(a 5-way matrix, one job per target):

- triggers on **every push and pull request** (same `on:` as the rest of CI);
- `ubuntu-latest`, Rust **nightly** (libFuzzer + ASan require it),
  `cargo-fuzz` pinned to **0.13.2**;
- `cargo fuzz run <target> -- -max_total_time=600 -rss_limit_mb=2560`
  — a 10-minute run per target;
- a crash **fails the job**; `fuzz/artifacts/` (repro inputs) is uploaded
  per-target as a workflow artifact (30-day retention) on failure.

The first green/red result of each matrix job is the authoritative
coverage-guided evidence; record it in the results section below when it
lands. **Not verified from the S1.5-P16 dev host**: the CI matrix itself
runs on GitHub infrastructure that this Windows machine cannot observe —
the checklist item «CI extended» is verified by the workflow file diff,
not by a CI run.

## Running on a capable host (Linux, macOS, MSVC)

```bash
cargo install cargo-fuzz
cargo fuzz run canonical_decode -- -max_total_time=600
cargo fuzz run host_functions -- -max_total_time=600
cargo fuzz run gas_metering -- -max_total_time=600
cargo fuzz run storage_trie -- -max_total_time=600
cargo fuzz run differential_wasmi_vs_wasmtime -- -max_total_time=600
```

Requires a nightly toolchain; libFuzzer + ASan provide coverage guidance and
crash detection. Corpus: `fuzz/corpus/<target>/`; artifacts (crashes):
`fuzz/artifacts/`.

## Why the dev machine (Windows) cannot run cargo-fuzz

The coverage-guided run is **not possible on the S1-P19/S1.5-P16 dev
machine** — the reason («причина фиксации» per the checklists), unchanged
since S1-P19:

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
   `__builtin_function_start`; GNU g++ cannot compile it by design (the
   machine has MSYS2, but `g++` lacks the required intrinsics).
   Additionally `getrandom` (via wasmtime, S1.5-P16) needs `dlltool.exe`
   on windows-gnu, which lives in `C:\msys64\mingw64\bin` and must be on
   `PATH` even for a plain `cargo check` of the fuzz crate.

**Fallback executed instead** (same contracts, no coverage guidance):

- `cargo run --example canonical_decode_soak` — decoders (S1-P19);
- `cargo run -p strangecoin-vm-wasm --example vm_fuzz_soak` — the four VM
  surfaces (S1.5-P16); `SOAK_SECONDS=<n> SOAK_PHASE=host|gas|storage|diff|all`.

Both use a seeded xorshift64* (deterministic, reproducible) and abort on
any panic. The differential phase of `vm_fuzz_soak` additionally runs
wasmtime as the oracle, so it also covers the «same output or both fail»
checklist item on hosts without cargo-fuzz.

A 10-minute *type-check* of the four S1.5-P16 targets (stub `fuzz_target!`
macro, one zeroed input each) also passes on this host; it validates that
the targets compile and their first iteration does not panic, not coverage.

## Results

### Coverage-guided (canonical_decode, S1-P19 / BUG-S0-023)

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
bounds-checked (`Result`), every decoder call site propagates errors, and
the per-transaction length check uses the live offset.

### Fallback soaks, four VM targets (S1.5-P16)

Filled in by the S1.5-P16 verification run on the dev host:

| Date | Phase | Seconds | Iterations | Panics | Note |
|------|-------|---------|------------|--------|------|
| 2026-10-17 | host | 600 | 140,189 | 0 | S1.5-P16, seeded xorshift64*, `SOAK_PHASE=host` |
| 2026-10-17 | gas | 600 | 501 | 0 | S1.5-P16; slow iterations are the infinite-loop family burning its fuel budget |
| 2026-10-17 | storage | 600 | 10,457 | 0 | S1.5-P16 |
| 2026-10-17 | diff | 600 | 1,730 | 0 | S1.5-P16; wasmi vs wasmtime, 0 divergences |

Coverage-guided evidence for the four new targets comes from the CI `fuzz`
matrix; it is **not verified from this host** (no cargo-fuzz — reasons
above), same as `canonical_decode` in S1-P19.

## Relationship to THREAT_MODEL.md

- `host_functions` / `gas_metering` / `differential_*` → threat **V-21**
  (malicious WASM): wasmi sandbox + host-function whitelist + gas metering
  are the mitigations; fuzzing is the verification that the boundary holds
  on adversarial input.
- `storage_trie` → invariant **#4** (apply/unapply are clean inverses) and
  the Stage 1.5 extension of invariant **#19** (contract storage under the
  state root).

S1.5-P18 folds the outcomes into `docs/security/THREAT_MODEL.md`
Monitoring.
