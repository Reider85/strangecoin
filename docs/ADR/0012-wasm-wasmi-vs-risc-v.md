# ADR-0012: Smart-contract VM — WASM (wasmi) vs RISC-V (ckb-vm)

## Status

Accepted (2026-10-15) — S1.5-P01

> **Numbering note:** the Stage 1.5 prompt (`analytics/prompt-stage1.5.md`) reserved
> ADR-0011 for this decision, but ADR-0011 was taken by the async-migration ADR
> (`docs/ADR/0011-async-migration.md`, BUG-S0-021, 2026-10-08). This decision is
> therefore numbered **ADR-0012**; the remaining Stage 1.5 ADRs shift accordingly
> (JSON ABI → 0013, gas metering → 0014, precompiles → 0015; ROADMAP3 to be updated
> when those ADRs land).

## Context

Stage 1.5 introduces smart contracts (`ROADMAP3.md` Stage 1.5, `ARCHITECT3.md` §3.9).
The chain needs an execution VM for contract bytecode with these forces in play:

- **Determinism** — every validator must compute identical execution results
  (threat V-21). The VM must not leak wall-clock time, randomness, or host I/O.
- **Gas metering** — malicious contracts must be bounded (infinite loops, memory
  bombs). The VM should support metering without a hand-written opcode table
  (ARCHITECT3 §15 anti-goal: "своя VM инструкция").
- **Clean core** — `strangecoin-core` stays 0-I/O; the runtime must live outside
  the core behind the `VmExecutor` trait (strangler pattern).
- **Fuzzing** — the runtime should have ready-made fuzzers and a track record of
  sandbox-escape audits.
- **Tooling** — contract authors need mature compilers (rustc, clang, Go,
  AssemblyScript) targeting the ISA.
- **Performance** — throughput of `validate_and_apply` matters; pure interpretation
  is slower than JIT but must remain acceptable at block gas limit 1M.

ARCHITECT3 §3.9 declares "WASM (wasmi)" without justification. This ADR records
the decision and its trade-offs before any runtime code lands (ADR-before-code
gate, S1.5-P05..P06).

## Decision

We will use **WASM** as the contract ISA, executed by the **wasmi** interpreter,
pinned at **`wasmi = "=2.0.0"`** (exact pin, no `^`/`~` — fuel/metering API churn
must not silently change gas semantics).

Key points:

- **Interpreter, not JIT** — wasmi is a pure-Rust interpreter: deterministic
  execution order, no tiering, no platform-dependent codegen. Consensus-critical
  code paths must not depend on host CPU features.
- **Runtime lives in `crates/strangecoin-vm-wasm/`** — separate crate, implements
  the `VmExecutor` trait from `strangecoin-core` (strangler). Core never imports
  wasmi.
- **Metering** — wasmi fuel/consumption API for instruction accounting plus
  per-host-call weights (details in the follow-up gas-metering ADR, planned 0014).
- **Version pin rationale** — `wasmi 0.31.0`, referenced by the original Stage 1.5
  prompt sketch, is **yanked** on crates.io; `0.31.2` is the last of the 0.31 line
  (2024-01, unmaintained relative to 1.0/2.0). Latest stable is `2.0.0`
  (2026-09-01) with the maintained fuel-consumption API. We pin `=2.0.0` and adapt
  the prompt's 0.31-era API sketches (`with_fuel_limit` etc.) to the 2.0 API
  (`Config::consume_fuel` + `Store::set_fuel`) during S1.5-P06/P09.

## Consequences

### Positive

- Deterministic interpreter; no JIT tiering or host-CPU-dependent codegen in
  consensus paths.
- Ready-made ecosystem: wasmi fuzzers (cargo-fuzz targets exist upstream),
  differential-testing against wasmtime (S1.5-P16), LLVM-backed compilers for
  contract authors (rustc `wasm32`, clang, Go, AssemblyScript).
- `no_std`-compatible runtime; sandbox has no I/O by default — host functions
  are the only channel (whitelist, threat V-21).
- Fuel/consumption metering ships with the engine — no hand-written opcode gas
  table (ARCHITECT3 §15 respected).
- Pure Rust, no C toolchain requirement for the runtime itself.

### Negative

- wasmi interpreter is slower than a JIT (wasmtime/wasmer); acceptable at the
  fixed 1M block gas limit, but throughput headroom is lower.
- Fuel/metering API has churned across wasmi majors (0.31 → 1.0 → 2.0); an
  engine upgrade can change gas semantics. Mitigation: exact version pin + gas
  constant changes only via SCIP after mainnet freeze.
- WASM linear memory is separate from the Sparse Merkle Tree state; contract
  state must be explicitly bridged through `storage_read`/`storage_write` host
  calls (integration in S1.5-P15).
- Interpreter overhead per instruction is higher than native execution; heavy
  crypto in contracts should go through precompiles/host calls, not WASM loops.

### Neutral

- Gas model: instruction counting (fuel) + host-call weights — details fixed in
  the follow-up gas-metering ADR (planned 0014), constants updatable via SCIP.
- ABI format (JSON ABI vs WIT) is a separate decision (planned ADR-0013).
- Precompiles list is a separate decision (planned ADR-0015).
- Differential testing vs wasmtime is planned (S1.5-P16); wasmtime is a
  test-only dependency, never a consensus dependency.

## Alternatives

### Alternative 1: RISC-V (ckb-vm)

- Pros: simpler ISA than WASM (easier formal audit); used in production by Nervos;
  deterministic execution; gas via instruction counting is straightforward.
- Cons: far smaller tooling ecosystem (no rustc/clang first-class targets for the
  exact ckb-vm profile); no ready-made fuzzers of the same maturity; contract
  author experience much worse; would diverge from ARCHITECT3 §3.9 without
  offsetting gains for this project's stage.
- Why not chosen: tooling and fuzzing maturity outweigh auditability edge; WASM
  is the ecosystem default for smart-contract VMs outside Ethereum.

### Alternative 2: EVM (revm/evmone ports)

- Pros: largest contract-author ecosystem; battle-tested gas model; tooling
  (Solidity, Foundry, Hardhat, MetaMask) is unmatched.
- Cons: EVM stack machine has no generic types, limited crypto surface, awkward
  for non-Ethereum address models; couples us to Ethereum semantics we do not need
  (account abstraction, EIP-1559 deferred); revm ports are typically non-deterministic
  across versions without strict pinning; conflicts with ARCHITECT3 §3.9 direction.
- Why not chosen: Strangecoin's crypto core (secp256k1 + blake3 + SMT) does not
  benefit from EVM opcode compatibility; the migration cost of Ethereum-shaped
  semantics exceeds the tooling benefit at MVP stage.

### Alternative 3: Move VM

- Pros: resource-oriented programming prevents asset duplication by construction;
  formal verification story (Move Prover).
- Cons: production-grade runtime effectively tied to the Diem/Libra lineage;
  smaller external ecosystem; resource model does not map onto the account/SMT
  model chosen for Stage 1.5; tooling beyond Aptos/Sui forks is thin.
- Why not chosen: runtime dependency risk and poor fit with the existing account
  model.

### Alternative 4: Custom VM (own ISA)

- Pros: full control over opcodes, gas, and state access; minimal binary size.
- Cons: explicit ARCHITECT3 §15 anti-goal; no fuzzers, no LLVM backend, no
  compiler ecosystem; years of engineering to reach parity; consensus-bug risk
  with no external audit trail.
- Why not chosen: anti-goal; cost prohibitive.

## Related

- `ARCHITECT3.md` §3.9 (VM subsystem), §15 (anti-goals)
- `ROADMAP3.md` Stage 1.5 (WASM smart contracts)
- `analytics/prompt-stage1.5.md` S1.5-P01 (this ADR), S1.5-P05..P06 (trait + crate)
- `docs/ADR/0011-async-migration.md` — occupies the ADR-0011 number (renumber note)
- Planned: ADR-0013 (JSON ABI vs WIT), ADR-0014 (gas metering), ADR-0015 (precompiles)
- `crates/strangecoin-core/src/vm/traits.rs` — `VmExecutor` trait (S1.5-P05 target)
- SCIP process (`docs/SCIP/scip-0000-process.md`) — post-freeze changes to gas
  constants or runtime semantics
