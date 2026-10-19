# ADR-0013: Contract ABI — JSON ABI (Ethereum-style) for MVP vs WIT

## Status

Accepted (2026-10-15) — S1.5-P02

Amended (2026-10-19) — S1.5-FBP03: calling convention for WASM exports
(selector-based dispatch, `main()` fallback, `sc::input_read`), see the
dedicated section below.

Amended (2026-10-19) — S1.5-FBP04: constructor calling convention
(`"constructor"` export, `init_args` calldata, `CreateTx.gas_limit`
budget, failed-receipt rollback), see the dedicated section below.

Amended (2026-10-19) — S1.5-FBP06: lazy contract storage access
(`SharedContractStorage` = `Arc<RwLock<ContractStorageMap>>`, per-frame
overlay unchanged), see the dedicated section below.

> **Numbering note:** reserved as ADR-0012 in `analytics/prompt-stage1.5.md`;
> renumbered to **ADR-0013** because 0012 is taken by the WASM-VM decision
> (`docs/ADR/0012-wasm-wasmi-vs-risc-v.md`). See the numbering note there.

## Context

Stage 1.5 contracts need an interface description format (functions, types,
events, errors) so that SDKs, explorers, test tooling, and (at Stage 4)
JSON-RPC `eth_*` bridges can encode/decode calls without hand-written parsers.
`ARCHITECT3.md` §3.9 states "простой JSON ABI для MVP, WIT migration на Stage 4+
когда tooling дозреет" without recording the trade-offs.

Forces at play:

- **Type safety** — WIT (Wasm Component Model) is strongly typed and
  W3C-standardized; JSON ABI relies on convention and stringly-typed selectors.
- **Tooling maturity** — Ethereum JSON ABI has a decade of tooling (ethers-rs,
  alloy, viem, Foundry, Hardhat); WIT tooling (wit-bindgen, component model)
  is younger and still moving.
- **Forward compatibility** — whatever we ship in MVP must be migratable without
  a hard fork of contract state; the format needs an explicit version field.
- **Stage 4 goal** — JSON-RPC `eth_*` compatibility (ROADMAP3 Stage 4) is
  trivially served by an Ethereum-shaped ABI and awkward under WIT.
- **Consensus surface** — the ABI itself is not consensus-critical (calldata is
  opaque bytes on-chain), but the canonical *encode/decode* rules used by the
  SDK and by `call` payload construction must be deterministic; a future WIT
  migration is gated like any other protocol change (activation height).

## Decision

We will use **JSON ABI (Ethereum-style)** for the MVP, with a mandatory
**`format_version`** field to enable a later WIT migration via activation height.

Key points:

- **Format:** a JSON document listing entries with
  `name`, `type` (`"function" | "event" | "error"`), `inputs`, `outputs`,
  `state_mutability` — same shape as Solidity `abi.json`.
- **Mandatory version field:** every ABI document carries
  `format_version: "json-v1"`. The MVP implements exactly `"json-v1"`;
  a future `"wit-v2"` is introduced by SCIP + activation height, not silently.
- **Selectors:** function selector = first 4 bytes of the hash of the canonical
  signature string (as Ethereum); exact hash function fixed in S1.5-P12
  (implementation prompt) — sketched here, not normative.
- **Scope:** ABI describes the *call payload* of `CallTx` (S1.5-P08) and event
  `Log` topics/data (S1.5-P14). It does not change tx serialization.
- **Full normative encode/decode spec** is deferred to S1.5-P12 (JSON ABI
  encode/decode + WIT placeholder); this ADR fixes the *choice*, not the byte
  rules.

### JSON ABI schema sketch (non-normative)

```json
{
  "format_version": "json-v1",
  "name": "Counter",
  "type": "contract",
  "entries": [
    {
      "type": "function",
      "name": "increment",
      "inputs":  [{ "name": "by", "type": "u64" }],
      "outputs": [{ "name": "",  "type": "u64" }],
      "state_mutability": "nonpayable"
    },
    {
      "type": "event",
      "name": "Incremented",
      "inputs": [
        { "name": "by",     "type": "u64",  "indexed": false },
        { "name": "caller", "type": "bytes32", "indexed": true }
      ]
    },
    {
      "type": "error",
      "name": "Overflow",
      "inputs": [{ "name": "at", "type": "u64" }]
    }
  ]
}
```

`state_mutability`: `"pure" | "view" | "nonpayable" | "payable"` (Ethereum
semantics: `view`/`pure` reject state-changing calls; `payable` accepts value).

## Calling convention (amended S1.5-FBP03)

How calldata reaches a WASM contract's code (implemented in
`strangecoin-vm-wasm`, `run_frame`):

- **Selector-based dispatch** — when a frame carries at least 4 calldata
  bytes and the callee has a registered `ContractAbi`, the first 4 bytes
  are matched via `entry_by_selector`; a hit names the WASM export to
  invoke (`entry.name`). A hit with **no matching export is a contract
  bug** and fails the frame with `MissingExport` — no silent fallback.
- **`main()` fallback** — no registered ABI, fewer than 4 calldata bytes,
  or no entry for the selector: the legacy `main` export runs (pre-ABI
  single-entry convention; backward compatible).
- **Export signatures** — probed in order `(i32, i32) -> i32`,
  `(i32, i32) -> ()`, `() -> i32`, `() -> ()`. The `(ptr, len)` pair
  points at a host-staged copy of the full calldata at the tail of the
  module's linear memory (grown as needed; the copy is unmetered because
  calldata is bounded by the transaction size). Zero-argument exports
  reach calldata via the `sc::input_read(out_ptr, out_len) -> i32` host
  function (bytes written; `-1` buffer too small; `0` empty; gas 3,
  ADR-0014). An export present but matching none of the four signatures
  fails with `TypeMismatch`.
- **Nested calls** — the same convention applies to every
  `call`/`staticcall`/`delegatecall` child frame: the nested `data`
  argument is the child frame's calldata and is dispatched identically.
- **Constructor** — the `Create` tx runs the WASM export named
  `"constructor"` with `init_args` as calldata; contracts without that
  export keep deploy-without-init semantics. Finalized by the
  S1.5-FBP04 amendment below.

## Amendment: constructor convention (2026-10-19, S1.5-FBP04)

Deploy-time execution of `CreateTx` (implemented in
`strangecoin-vm-wasm::WasmiExecutor::execute_constructor` +
`strangecoin-core::state::apply_create`):

- **Invocation** — the runtime invokes the WASM export named
  `"constructor"`; `CreateTx.init_args` is the calldata, staged exactly
  like a call frame's (`(ptr, len)` argument pair, `sc::input_read`
  reachability). The MVP resolves the export **by name**;
  `CreateTx.constructor_params` (declared types of `init_args`) is on
  the wire for a future ABI-driven selector (`method_selector(
  "constructor", &constructor_params)`) but is not consulted yet.
- **Budget** — `CreateTx.gas_limit` (S1.5-FBP04 wire field) is the
  constructor frame's fuel; `0` means "no init budget" and is legal for
  legacy deploys. Consensus caps it at `BLOCK_GAS_LIMIT` in
  `verify_transaction`, and `enforce_block_gas_limit` reserves it like a
  `Call` gas limit.
- **Skip before instantiation** — a module that does not export a
  `constructor` function deploys without init: the runtime checks the
  module's export section *before* instantiation (so legacy modules —
  including ones whose declared memory the resource limiter would deny
  at instantiate time — keep deploying) and reports `Success` with
  `gas_used = 0`.
- **Success** — staged storage writes flush to
  `State.contract_storage`, the touched `storage_root`s recompute
  (invariant #19 extension), the `endowment` moves sender → contract,
  and the receipt carries the metered gas and constructor logs.
- **Failure** — failed-receipt semantics (mirrors `apply_call`; the
  block still applies): the deployment rolls back completely (no
  account, no storage, no endowment movement), the sender nonce stays
  consumed, and the receipt records `OutOfGas` (gas burned =
  `gas_limit`) or `Failed` via `Receipt::from_vm_error`. `unapply`
  reverses a successful constructor (storage undo + endowment refund)
  and skips the refund for a failed receipt.

## Amendment: value settlement & receipt audit trail (2026-10-19, S1.5-FBP05)

Nested value transfers used to settle only at the VM level — the
frame's account view moved the balance, but `apply_call` persisted
just the top-level `CallTx.value`, so a `call` chain A → B → C lost
every intermediate leg on-chain. This amendment fixes settlement and
defines the audit trail:

- **On-entry crediting** — a top-level `Call` frame's entry transfer
  (`ctx.caller` → `ctx.callee`, `ctx.value`) settles **inside** the
  executor before the callee's code runs (`run_frame`, `FrameEntry::
  Selector`, `depth == 0`): the contract observes the incoming value in
  its balance immediately and can forward it (withdraw pattern).
  Constructor frames keep the FBP04 convention — `apply_create` credits
  the `endowment` to the contract account before the frame starts, so
  the constructor likewise observes its balance.
- **Nested `call` settlement** — `nested_call` moves `value` from the
  frame's `storage_address` account (for `delegatecall` frames that is
  the *delegating* parent — callee code spends the parent's balance,
  mirroring Ethereum's `address(this)`) to the callee's account view
  before the child runs. Reverted/failed children never merge their
  accounts back — the move is reversed implicitly. `delegatecall` and
  `staticcall` never move value; a non-zero amount from a malicious
  module fails the child (`STATUS_FAILED`). The reentrancy/depth gates
  (S1.5-P13) run **before** any value transfer.
- **Audit trail** — every settled move is recorded in commit order in
  `ExecutionResult::value_transfers` (`ValueTransfer { from, to,
  amount }`), mirrored into `Receipt::value_transfers`, and committed
  to `state.balances` on `Success` by `apply_call`/`apply_create` with
  an undo record per txid (`State::value_transfers_undo`, accumulating
  across blocks like `State::storage_undo` so chained rollbacks of
  non-tip blocks can reverse the trail). `unapply` reverses the list in
  reverse commit order. Transfer-only receipts (plain `TransferTx`)
  carry their single move; coinbase/genesis mints carry none.
- **Receipt encoding** — `serialize_receipt` ends with the trail
  (count + per-transfer from/to/amount). Receipts are derived data —
  never persisted or transmitted, only recomputed from the chain — so
  the layout change does **not** bump `FORMAT_VERSION`; the version
  byte for the receipts domain gets locked together with the header
  commitment (`receipts_root`, S1.5-FBP07).
- **Settlement is sequential** — commit and reverse read live balances
  per transfer, so a self-move (`from == to`) nets to zero (this also
  fixed a latent mint bug in the transfer-only `Call` fast path, which
  previously read both balances before writing either).

## Amendment: lazy contract storage access (2026-10-19, S1.5-FBP06)

`apply_call`/`apply_create` used to deep-clone the entire chain-wide
`State.contract_storage` into `ExecutionContext.storage` on every call
frame — O(total storage) per `CallTx` (and per nested frame via
`ctx.clone()`), which is prohibitive once contracts accumulate
100K+ keys. This amendment makes the persisted snapshot shared:

- **Shared handle** — `State.contract_storage` and
  `ExecutionContext.storage` are both `SharedContractStorage` =
  `Arc<RwLock<ContractStorageMap>>`. A call frame gets an `Arc::clone`
  (O(1)); nested frames inherit it through `ctx.clone()` at the same
  cost. `Arc` (not `Rc<RefCell>`) keeps `State`/`ExecutionContext`
  `Send + Sync` — required by the multi-threaded node (tokio workers,
  facade behind `RwLock`).
- **Read-only during execution** — the shared base is never mutated
  while the VM runs: `storage_read` falls through the per-frame overlay
  to the base under a short-lived `read()` guard; `storage_write` stages
  into the overlay only. Revert semantics are unchanged — a failed
  frame's overlay is dropped.
- **In-place flush** — on `Success`, `flush_storage_writes` merges the
  overlay into the shared base under a single `write()` guard and
  recomputes each touched `storage_root` (invariant #19 extension);
  `unapply_call_storage` restores pre-images the same way (invariant
  #4). The lock is never held across `vm.execute`.
- **State cloning stays deep** — `State::clone` (used per block by
  `apply_block_inner`) copies the map into a **fresh** `Arc`, and
  `StateCache::to_state` likewise never shares the cache's handle: a
  failed mid-block apply must not mutate the committed snapshot through
  the shared base.
- **Not in this change** — iterator-based lazy reads against a real
  KV store (Stage 3, RocksDB); the `storage_write` size-cap check still
  sums the contract's persisted bytes per write (O(contract storage)).

## Consequences

### Positive

- Direct path to Stage 4 `eth_*` JSON-RPC compatibility — same ABI document
  shape, same selector convention; SDK code ports with minimal changes.
- Mature ecosystem: existing Ethereum ABI coders (alloy/ethers-rs, viem) can be
  reused or mirrored; contract authors already know the format.
- Human-readable and diff-friendly; trivial to embed in docs and test fixtures.
- No extra runtime dependency (plain `serde_json` in tooling; canonical binary
  encoding of calldata stays in core).

### Negative

- Less type-safe than WIT: type errors surface at encode/decode time, not at
  compile time; mitigated by SDK-side codegen from the ABI document later.
- Ethereum-style selectors inherit Ethereum's weak typing (4-byte collisions);
  acceptable at MVP scale, revisited if collisions are observed.
- `format_version` migration path is mandatory overhead: every consumer must
  branch on the field; a sloppy consumer that ignores it will break at WIT
  migration.

### Neutral

- WIT migration remains planned for Stage 4+ (ROADMAP3), gated by SCIP +
  activation height; `"json-v1"` consumers keep working until then.
- Calldata on-chain is opaque bytes either way — this decision does not touch
  consensus rules, serialization, or gas.
- Cairo ABI / Move ABI ecosystems are not targeted; no compatibility promise.

## Alternatives

### Alternative 1: WIT (Wasm Component Model)

- Pros: W3C standard; strongly typed (records, variants, resources); compile-time
  checking via wit-bindgen; the long-term right answer for component interop.
- Cons: tooling still young (2023+ standard, moving specs); no `eth_*` bridge
  story for Stage 4; heavier developer learning curve; fewer ready-made SDK
  coders; component-model support in wasmi is not the MVP path.
- Why not chosen: too early for MVP; keep as migration target via
  `format_version` once tooling stabilizes (explicit Stage 4+ plan).

### Alternative 2: Cairo ABI

- Pros: resource-oriented types map well to assets; concise.
- Cons: designed for the Cairo VM/Starknet stack, not wasmi; no tooling outside
  that ecosystem; would couple ABI to a VM we are not using.
- Why not chosen: VM mismatch.

### Alternative 3: Move ABI

- Pros: first-class resource types; formal-verification friendly.
- Cons: same VM-mismatch problem as Cairo; production tooling tied to
  Aptos/Sui lineage; poor fit for the account/SMT model chosen in Stage 1.5.
- Why not chosen: VM and ecosystem mismatch.

### Alternative 4: Custom binary ABI

- Pros: maximal compactness; zero ambiguity if fully specified; smallest
  decoder surface.
- Cons: zero off-the-shelf tooling; every SDK/explorer/test must be written from
  scratch; higher risk of subtle encode/decode divergences between implementations.
- Why not chosen: tooling cost vastly exceeds the bytes saved at MVP stage;
  JSON + canonical binary calldata already gives compact on-chain form.

## Related

- `docs/ADR/0012-wasm-wasmi-vs-risc-v.md` — VM decision (renumber origin)
- `ARCHITECT3.md` §3.9 (VM subsystem), §15 (anti-goals: WIT deferred)
- `ROADMAP3.md` Stage 1.5 (JSON ABI MVP), Stage 4 (`eth_*` compatibility)
- `analytics/prompt-stage1.5.md` S1.5-P02 (this ADR), S1.5-P12 (encode/decode impl)
- Planned: ADR-0014 (gas metering), ADR-0015 (precompiles)
