# Contributing to Strangecoin

Thanks for helping make Strangecoin better. This document is the single source of truth for how contributions are made here: environment setup, workflow, commit conventions, review bar, and the rules that keep the chain deterministic and safe.

---

## Table of contents

1. [Ways to contribute](#ways-to-contribute)
2. [Read this first](#read-this-first)
3. [Development setup](#development-setup)
4. [Workflow](#workflow)
5. [Commit conventions](#commit-conventions)
6. [Checks before you push](#checks-before-you-push)
7. [Code style](#code-style)
8. [Testing](#testing)
9. [Dependencies and pinning](#dependencies-and-pinning)
10. [Determinism rules](#determinism-rules)
11. [Consensus and protocol changes](#consensus-and-protocol-changes)
12. [Pull request checklist](#pull-request-checklist)
13. [Branching and releases](#branching-and-releases)
14. [Security](#security)
15. [License](#license)

---

## Ways to contribute

- **Bug reports and feature requests** — open a GitHub issue. Include Rust version, OS, and reproduction steps for bugs.
- **Pull requests** — see the workflow and checklist below.
- **Security vulnerabilities** — do **not** open a public issue. Follow [`docs/security/SECURITY.md`](docs/security/SECURITY.md): email `security@strangecoin.io`, 48-hour acknowledgment, 90-day coordinated disclosure, paid bounty per [`docs/security/BOUNTY.md`](docs/security/BOUNTY.md) ($1k / $10k / $100k tiers).
- **Consensus divergence, fuzz crashes, wasmi/wasmtime differential findings** — these are explicitly in bounty scope and always welcome.

---

## Read this first

| Document | Why |
|----------|-----|
| [`AGENTS.md`](AGENTS.md) | Developer guide: architecture, gotchas, build commands, stage status, lock ordering |
| [`analytics/ARCHITECT3.md`](analytics/ARCHITECT3.md) | Target architecture; §17 is the PR review checklist this project runs |
| [`analytics/ROADMAP3.md`](analytics/ROADMAP3.md) | Stage plan and backlog |
| [`docs/ADR/`](docs/ADR/) | All accepted architectural decisions (0001–0026) |
| [`docs/security/THREAT_MODEL.md`](docs/security/THREAT_MODEL.md) | STRIDE model, V-1..V-42 attack vectors |
| [`docs/stage2/INVARIANTS_ENFORCED.md`](docs/stage2/INVARIANTS_ENFORCED.md) | The 22 invariants a PR must not regress |
| [`Changelog.md`](Changelog.md) | What shipped, per tagged stage |

**Rule of thumb:** if your change touches architecture, consensus, cryptography, storage layout, or wire format, there must be an ADR (and a SCIP for protocol changes) — either an existing one you follow, or a new one you propose *before* the code.

---

## Development setup

**Prerequisites:** stable Rust 1.74+ (Linux, macOS, Windows). CI runs stable and beta on ubuntu/macos/windows.

```bash
git clone https://github.com/Reider85/strangecoin.git
cd strangecoin

# Enable the commit-message hook (once per clone)
git config core.hooksPath .githooks

cargo check
cargo test
```

### Windows host preamble (required)

On Windows (`x86_64-pc-windows-gnu`), every shell invocation that runs cargo **must** start with:

```powershell
$env:PATH = "C:\msys64\mingw64\bin;$env:PATH"; cargo <...>
```

- Without it, cc-rs build scripts (blake3, secp256k1-sys, wasmtime, eframe, rocksdb) fail with `error calling dlltool 'dlltool.exe': program not found` or a silent `gcc exit 1`.
- **Never** put shim directories (e.g. `target/dlltool-shim`) ahead of msys64 in PATH — a swapped `as.exe` breaks cc-rs silently.
- **Never** set `CC_*` / `AR_*` / `CFLAGS` env vars — cc-rs does `rerun-if-env-changed`, which invalidates every C build fingerprint and forces a multi-minute full rebuild.

Long test runs (30+ minutes, e.g. rocksdb C++ builds) should be launched detached (`Start-Process cmd /c '... > target\x.log 2>&1'`) — see `AGENTS.md` §Common Gotchas.

---

## Workflow

1. **Fork** and create a feature branch off `master`.
2. **Enable git hooks** (`git config core.hooksPath .githooks`) — see below.
3. **Decide the artifact class:**
   - Architecture / consensus / crypto / storage / wire change → **ADR first** (`docs/ADR/0001-template.md`; ADRs are immutable once accepted — only superseded by a new ADR).
   - Protocol-level change → **SCIP** (`docs/SCIP/`, Idea → Draft → Review → Activation → Finalized).
   - Bug fix → reference the issue / `BUG-SN-NNN` id if one exists.
4. **Write code** following the style and determinism rules below.
5. **Run checks** (see next section) — all green before pushing.
6. **Open a PR** against `master` with: motivation, design summary, ADR/SCIP references, and test evidence (commands you ran + results).
7. **Respond to review** — each review round should be a new commit or a clearly described force-push (state it in the PR).

### Documentation updates (FBP10)

`AGENTS.md`, `Changelog.md`, and `README.md` are updated **only in the same commit as the artifact they describe**. Do not announce "stage complete" in prose — completion is declared by an **annotated git tag** only. Non-locally-verifiable facts (CI runs, fuzz stats, signature verification) must be marked as "not verified locally + why", never silently omitted.

---

## Commit conventions

**Format:** `[<id>] <area>: <short description>`

```
[S3-P05] storage: schema.rs KeyPrefix encoding + versioning v1
[S2-P22] tests: net integration (noise/gossip/compact_blocks/erlay/scoring/rate_limit)
[BUG-S0-002] docs: ...
docs: stage3 prompt backlog
```

- `<id>` is the prompt/issue id: `[S3-P01]`..`[S3-P15]`, `[S2-PNN]`, `[S1.5-PNN]`, `[BUG-SN-NNN]`, or no id for docs/chore commits.
- **One logical change = one commit.** Do not mix an ADR, unrelated refactors, and a feature in one commit.
- The `commit-msg` hook (`githooks/commit-msg`) validates this format and **warns** (never blocks) on mismatches.
- Author identity: commits are made as the committer configured in git; PRs from forks keep your own identity.

---

## Checks before you push

Run exactly these — the project does not require extra full release builds or repo-wide re-runs unless the PR calls for it:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check
cargo test --workspace
```

Notes:

- **Windows timing** (measured): `cargo check --workspace` 2–4 min warm; clippy ~1 min; full `cargo test --workspace` ~10–12 min. Run tests in chunks (`-p strangecoin` → `-p strangecoin-core` → `-p strangecoin-vm-wasm` → `-p strangecoin-net`) with output redirected to a file (`cmd /c "cargo ... > target\x.log 2>&1"`) — piping through PowerShell kills long runs.
- **fmt drift:** if `cargo fmt --all` produces diffs only because the rustfmt channel changed, commit formatting as a separate style commit.
- CI additionally runs **source gates** (no direct `.balances` mutations outside whitelisted files; every `#[allow(dead_code)]` / `#[allow(clippy::await_holding_lock)]` needs a `//` justification on the previous line), a **3-OS × stable/beta test matrix**, **release build**, **coverage**, and **9 fuzz targets × 600s**.

---

## Code style

- **Rust edition 2021.** Format with `cargo fmt`; no style debates in review.
- **No comments unless explaining *why*** (trade-offs, deviation from a spec, invariant rationale). Code should be self-documenting.
- **Logging:** `tracing` macros with structured fields (`tracing::info!(target = "net::noise", peer_id = %id, ...)`). No `println!`/`eprintln!` in library/binary code — CLI machine-readable stdout output is not logging and is allowed.
- **Errors:** `thiserror` for library error types; `anyhow` only at application boundaries.
- **Pure core, impure shell:** `crates/strangecoin-core` has **zero I/O** — no `std::fs`, `std::net`, `tokio`, or database code. This is enforced by CI audits. The binary, VM, network, and storage crates are thin shells around it.
- **Strangler pattern:** new subsystems go into their own crate (`crates/strangecoin-*`); the root `src/` stays an orchestrator. Cross-crate cycles are broken with traits (`BlockchainView`, `EventSink`, `VmExecutor`).
- **Lock ordering** (see `src/error.rs`): `blockchain` → `peer_manager` → `connection_pool` → `wallet`. Never hold an outer lock while taking an inner one in the reverse order.
- **Tracing over panics** in network paths: unknown/future message types are logged and ignored, never crashed on; malformed *known* messages are errors.

---

## Testing

- **Unit tests** live in `#[cfg(test)]` modules next to the code.
- **Integration tests** live in top-level `tests/` (real processes/ports where applicable — no mocked consensus paths for e2e behavior).
- **Property tests** with `proptest` for consensus-critical logic: canonical serialization, validation, emission, chain selection, state round-trip, merkle, witness, ABI types.
- **Storage tests** create temporary databases in the system temp dir, never in `target/debug/`.
- **Network tests** use `NETWORK_TEST_LOCK` (they write `network.json` next to the test binary) and real TCP on random ports.
- **Fuzz targets** live in `fuzz/` (9 targets). `cargo-fuzz` runs in CI; on Windows hosts it does not build — use `cargo run --example canonical_decode_soak` as local smoke.
- **Determinism:** time-dependent tests use injected clocks (`tick_at`, `check_rate_at`, `reconcile_at`) — **zero sleeps** in tests.

A new behavior needs a test. A fixed bug needs a regression test.

---

## Dependencies and pinning

- Minimize external crates; prefer stdlib. New dependencies must be MIT/Apache-2.0 and actively maintained.
- **Consensus-, crypto-, network-, and storage-adjacent crates are exact-pinned** with `=` (e.g. `snow = "=0.10.0"`, `wasmi = "=2.0.0"`, `ed25519-dalek = "=2.0"`, `rocksdb = "=0.25.0"`). After mainnet freeze, changing a pinned version requires a **SCIP + activation height** so all nodes upgrade in lockstep.
- The full pin table lives in `AGENTS.md` (§Stage 2 / §Stage 3 Pin Policy).
- Vendored code (`vendor/minisketch-rs`) is SCIP-gated; do not flip features on casually.

---

## Determinism rules

Consensus must be byte-identical across all nodes. Every new function on a consensus-, storage-, or network-path must satisfy:

1. **No `SystemTime::now()` / `Instant::now()`** in encoding, validation, pruning, snapshot, handshake, scoring, or sketch paths. Time enters only through block headers or explicit injected-clock edges (`tick_at`-style APIs). Wall clock at file-load edges (ban expiry on `network.json`) is ban policy, not consensus state — document it if you add one.
2. **No `thread_rng()` / `OsRng`** in hot paths. Keys derive from the keystore long-term seed via HKDF; `OsRng` is allowed only for one-time key generation at node startup.
3. **Key encoding is pure:** big-endian `u64` for heights/nonces so lexicographic order equals numeric order; sorted iteration stays deterministic.
4. **Deterministic ordering:** anything iterated for wire output or persisted state (peers, accounts, snapshot entries, inventory) is sorted by a deterministic key first — never by arrival time or `HashMap` order.
5. **Reproducibility:** round-trip and codec tests must pass identically across runs.

Violations of these rules are review-blocking.

---

## Consensus and protocol changes

- **Consensus rules change only via SCIP + activation height** (`docs/SCIP/`, `crates/strangecoin-core/src/governance/scip.rs`). No silent rule changes, ever.
- **Canonical serialization** (`crates/strangecoin-core/src/serialize.rs`, blake3) is the wire truth. `serde_json` is for config/tooling only. Changing canonical bytes is a hard fork.
- **Grant blocks** (`allow_grant_blocks=true`) are regtest/legacy-migration only.
- **Wire compatibility:** deprecated messages stay deleted (Stage 2 removed the v0.8.6 text protocol); forward compatibility is handled by `UnknownMessage` → log + ignore.
- After mainnet freeze, anything on the pinned-dependency list or the consensus path goes through SCIP with an activation height.

---

## Pull request checklist

Derived from `analytics/ARCHITECT3.md` §17 — reviewers run this:

```
[ ] Pure core: serialize/consensus/state/economics/governance — 0 I/O in strangecoin-core
[ ] Invariants 1–22 not regressed (docs/stage2/INVARIANTS_ENFORCED.md)
[ ] Threat model: new attack vector? Mitigation + doc update (docs/security/THREAT_MODEL.md)
[ ] New events go through EventBus, not direct Mutex poking
[ ] Strangler respected: subsystem in its own crate; root src/ stays orchestrator
[ ] Sync/network cycles broken via BlockchainView / EventSink traits
[ ] VM trait in core, runtime (wasmi) separate
[ ] Tie-breaking unchanged: work → earliest timestamp → lowest hash
[ ] Storage schema versioned + migration path (ADR-0024)
[ ] Governance: SCIP + activation height for any consensus change
[ ] Determinism rules above satisfied (no wall clock / RNG in hot paths)
[ ] Anti-goals respected (analytics/ARCHITECT3.md §15)
[ ] ADR for architectural decisions, SCIP for protocol changes
[ ] Tests: unit + proptest (where consensus-critical) + integration for the behavior
[ ] cargo fmt / clippy -D warnings / check / test green locally
[ ] Docs updated in the same commit as the artifact (FBP10), or not at all
[ ] Changelog entry only if this ships in a release
```

---

## Branching and releases

- **`master`** — the only long-lived branch. PRs require passing CI.
- **Stages** are delivered as tagged releases: `v1.0.0-stage0`, `v1.1.0-stage1`, `v1.5.0-stage1.5`, `v2.0.0-stage2`, `v3.0.0-stage3` (next).
- **Versioning:** SemVer on the workspace root; each stage bumps minor/major and gets a `Changelog.md` section with DoD evidence.
- **Releases** are built by `.github/workflows/release.yml`: 6-binary matrix (Linux/macOS/Windows × x86_64/aarch64), `SHA256SUMS`, SLSA L3 provenance, cosign keyless OIDC signatures. Details in `docs/security/REPRODUCIBLE_BUILDS.md`.
- **Tags are annotated** and created only after the stage's DoD is verified — never "declare done in prose".

---

## Security

- **No secrets in code or config.** Passwords and keys only via environment variables (`STRANGECOIN_WALLET_PASSWORD`) or the encrypted keystore. Node Ed25519 keys live under `keystore/node/`, wallet secp256k1 keys under `keystore/wallet/` — never in `config.toml`.
- **Report vulnerabilities privately** per [`docs/security/SECURITY.md`](docs/security/SECURITY.md). PGP key generation is an open ops gate; until it lands, treat the security email as unencrypted and prefer out-of-band contact for critical reports.
- **Safe harbor:** good-faith research within the documented scope is protected; see `SECURITY.md` §5.

---

## License

By contributing, you agree your contributions are licensed under the **Apache License, Version 2.0** (see [`LICENSE`](LICENSE) and [`docs/ADR/0027-license-change-to-apache-2.0.md`](docs/ADR/0027-license-change-to-apache-2.0.md)).

---

## Questions

Open a GitHub issue for design discussion. For anything security-sensitive, use the private channel in `docs/security/SECURITY.md`.
