# ADR-0014: Gas Metering Strategy — wasmi Fuel + Host-Call Weights

## Status

Accepted (2026-10-15) — S1.5-P03

> **Numbering note:** reserved as ADR-0013 in `analytics/prompt-stage1.5.md`;
> renumbered to **ADR-0014** (0011 = async migration, 0012 = WASM VM,
> 0013 = JSON ABI). See the numbering note in ADR-0012.

## Context

Without gas metering, a malicious contract can loop forever or allocate
unbounded memory and halt block validation (threat V-21). `ARCHITECT3.md` §3.9
declares "instruction counting + host-call weights (не таблица опкодов вручную)"
and a fixed 1M block gas limit, but records no rationale, no constants, and no
upgrade path. This ADR fixes the strategy before the metering implementation
(S1.5-P09) and host functions (S1.5-P10) land.

Forces at play:

- **Determinism** — gas accounting must be identical on every validator;
  wall-clock time is forbidden as a meter (ARCHITECT3 threat V-21).
- **Anti-DoS** — unbounded loops, memory growth, and expensive host calls
  (crypto, SMT updates) must all be payable; any un-metered channel is a DoS.
- **Auditability** — a hand-written opcode→gas table is an anti-goal
  (ARCHITECT3 §15: "своя VM инструкция"); metering should piggyback on the
  engine where possible.
- **Engine churn** — the wasmi fuel API changed across majors (0.31 → 1.0 → 2.0,
  see ADR-0012 pin rationale); the strategy must survive an engine upgrade
  without rewriting the gas model.
- **Post-freeze stability** — after mainnet freeze, gas constants are consensus
  rules; they may only change via SCIP + activation height.

## Decision

We will meter execution as **wasmi fuel (instruction accounting) + per-host-call
weights + explicit memory-growth weight**, under a **fixed 1M block gas limit**.

Key points:

- **Base instruction accounting** — wasmi's fuel/consumption API
  (`Config::consume_fuel(true)`, `Store::set_fuel` / `get_fuel` on the pinned
  2.0.0 engine, ADR-0012) charges 1 fuel per executed WASM instruction
  (`BASE_INSTRUCTION_GAS = 1`). No hand-written opcode table.
- **Host-call weights** — every host function (S1.5-P10) charges a fixed
  constant (sketch below) via `GasCounter::spend_host_call` *before* doing
  work; variable-size inputs add a per-byte/per-word term (sketched per host
  function in ADR-0015 precompile costs, mirrored for host calls).
- **Memory growth** — `memory.grow` is charged as a host-call-equivalent weight
  per 64KiB page (`HOST_CALL_MEMORY_GROW = 3`), preventing memory bombs within
  the fuel model.
- **Block gas limit** — `sum(tx.gas_used) <= 1_000_000` enforced in
  `block_executor::validate_and_apply` (invariant #15, first enforced at
  Stage 1.5). Fixed constant until EIP-1559-style limits (Stage 5).
- **Out-of-gas semantics** — fuel exhaustion traps the wasmi store →
  `VmError::OutOfGas`; the tx consumes its full `gas_limit`, state changes from
  the failed call frame are discarded (receipt status = reverted), logs are
  dropped.
- **Constants are consensus rules** — the table below is a sketch; normative
  values land with S1.5-P09/P10 code. After mainnet freeze any constant change
  is a SCIP (`docs/SCIP/scip-0000-process.md`) + activation height.

### Gas constants sketch (non-normative)

| Constant | Value | Rationale |
|---|---|---|
| `BASE_INSTRUCTION_GAS` | 1 | 1 fuel per wasmi instruction; engine-native, no table |
| `HOST_CALL_SECP256K1_VERIFY` | 3000 | libsecp256k1 verify ~µs-scale; mirrors precompile ecrecover (ADR-0015) |
| `HOST_CALL_BLS_VERIFY` | 12000 | BLS12-381 pairing-heavy; reserved until Stage 7 PoS |
| `HOST_CALL_BLAKE3` | 50 + 6 / 64B chunk | blake3 is fast; chunk term tracks input size |
| `HOST_CALL_STORAGE_READ` | 200 | overlay lookup + future SMT proof verification |
| `HOST_CALL_STORAGE_WRITE` | 5000 | SMT update + receipt/log cost; writes must hurt |
| `HOST_CALL_MEMORY_GROW` | 3 / page | 64KiB page; cheap but non-zero |
| `BLOCK_GAS_LIMIT` | 1_000_000 | fixed anti-DoS bound per block (invariant #15) |

## Consequences

### Positive

- No hand-written opcode gas table — instruction accounting is engine-native
  and survives compiler improvements of contract code paths.
- One uniform model (fuel units) for instructions, host calls, and memory;
  auditors reason about a single counter.
- Host-call weights make the expensive channels (crypto, SMT writes) payable —
  closes the classic "cheap infinite host call" DoS.
- Fixed block gas limit gives a hard worst-case bound on validation time per
  block (invariant #15).

### Negative

- Fuel API is engine-specific: a wasmi major upgrade can change charging
  granularity. Mitigation: exact version pin (ADR-0012) + differential tests
  (S1.5-P16); engine upgrades that alter gas semantics are SCIP-gated post-freeze.
- Instruction counting ignores microarchitectural cost variance (cache-hot vs
  cache-cold, big-int limbs inside WASM); acceptable because consensus requires
  determinism, not wall-clock fairness.
- Sketch constants are estimates; miscalibration may underprice an operation
  (DoS vector) or overprice it (UX friction). Mitigation: benchmarks before
  mainnet freeze; constants tunable until freeze, SCIP after.
- Recurring-block consumption strategy adds engine overhead; measured later in
  S1.5-P09 tests (infinite-loop must OOG in <1ms).

### Neutral

- Gas *price* / fee market economics are out of scope (EIP-1559 → Stage 5,
  `economics/fee_market.rs` stub); this ADR only fixes the *metering* model.
- Precompile gas costs live in ADR-0015 but share the same units and are
  charged through the same `GasCounter`.
- Refunds (unused gas returned) follow the usual rule: `gas_used` only is
  charged; full `gas_limit` is reserved up-front.

## Alternatives

### Alternative 1: Hand-written opcode gas table (Ethereum-style)

- Pros: fine-grained control per opcode; battle-tested model in Ethereum.
- Cons: explicit ARCHITECT3 §15 anti-goal; table must track every engine change;
  large audit surface; easy to drift from actual cost.
- Why not chosen: anti-goal; engine fuel gives the same DoS protection with a
  fraction of the audit surface.

### Alternative 2: Memory-only metering

- Pros: trivial to implement (gas = peak pages × factor).
- Cons: CPU-bound infinite loops with static memory are free → does not close
  threat V-21; useless without instruction accounting.
- Why not chosen: insufficient alone.

### Alternative 3: Time-based metering

- Pros: measures real resource directly; zero calibration work.
- Cons: non-deterministic across validators → consensus splits; banned by
  threat V-21 determinism requirement.
- Why not chosen: consensus-fatal nondeterminism.

### Alternative 4: Hybrid (instruction + memory + separate host meters)

- Pros: theoretically finer fidelity.
- Cons: three counters instead of one; more states in the receipt path; the
  "memory + host" parts are already expressible as weights in the fuel model —
  a separate counter adds complexity without new capability.
- Why not chosen: the chosen model already covers instructions, memory growth,
  and host calls in one unit.

## Related

- `docs/ADR/0012-wasm-wasmi-vs-risc-v.md` — wasmi pin (fuel API surface)
- `docs/ADR/0015-precompiles-list.md` — precompile gas costs (same units)
- `ARCHITECT3.md` §3.9 (gas metering), §15 (anti-goals), §6 threat V-21
- `ROADMAP3.md` Stage 1.5 (block gas limit 1M, invariant #15)
- `analytics/prompt-stage1.5.md` S1.5-P03 (this ADR), S1.5-P09 (metering impl),
  S1.5-P10 (host-call weights)
- SCIP process (`docs/SCIP/scip-0000-process.md`) — post-freeze constant updates
