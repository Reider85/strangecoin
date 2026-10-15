# ADR-0013: Contract ABI — JSON ABI (Ethereum-style) for MVP vs WIT

## Status

Accepted (2026-10-15) — S1.5-P02

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
