# ADR-0020: Noise Protocol Framework for P2P Transport Encryption

## Status

Accepted (2026-10-24) — S2-P01

## Context

Strangecoin's P2P transport (Stage 1) is raw TCP with no encryption or
authentication beyond application-level message framing. ARCHITECT3 §3.6
mandates «Noise-шифрование с Stage 2»; §6 threat **V-24 (Network MITM)**
requires forward secrecy + handshake auth. The node currently accepts
plaintext frames from any TCP peer — a network attacker can sniff txs/blocks
in transit, inject forged messages, or impersonate peers.

Forces at play:

- **MITM defense (V-24)** — every post-handshake frame must be AEAD-encrypted;
  a passive observer learns nothing; an active tamperer fails decryption.
- **Forward secrecy** — compromising a node's long-term static key must not
  decrypt previously recorded sessions. Ephemeral keys per handshake provide
  this property.
- **No certificate PKI** — P2P nodes join permissionlessly; there is no CA to
  issue X.509 certificates. TLS 1.3 would require either a private PKI
  (operational overhead, single point of compromise) or TOFU pinning that
  reimplements Noise's static-key binding anyway.
- **Determinism (V-21 extension)** — the handshake must be deterministic
  across restarts for a given keystore seed (Stage 2 Determinism Checklist:
  ephemeral keys derived from `keystore.long_term_seed` via HKDF, never
  `OsRng` in the handshake path).
- **Ed25519 identity separation** — EOA signatures stay secp256k1 (ADR-0001);
  internal node-to-node identity uses a separate Ed25519 keypair (ARCHITECT3
  §3.8) to keep the crypto surface cleanly divided.
- **Battle-tested crypto** — ARCHITECT3 §15 anti-goal: «Hand-written
  cryptographic primitives». The implementation must come from a maintained
  Rust crate, not bespoke crypto.

## Decision

**Noise Protocol Framework, XX handshake pattern, via the `snow` crate,
with ChaCha20-Poly1305 transport encryption and Ed25519 static keys for peer
identity.**

| Aspect | Decision |
|---|---|
| Protocol | Noise Protocol Framework, `Noise_XX_25519_ChaChaPoly_BLAKE2s` |
| Handshake pattern | XX — mutual auth without pre-shared keys, 3 round trips, ephemeral keys for forward secrecy |
| Transport cipher | ChaCha20-Poly1305 AEAD (via snow `TransportState`) |
| Handshake hash | BLAKE2s (noise spec default for XX_25519_ChaChaPoly) |
| Static key type | X25519 (noise DH); **node identity** bound via Ed25519 signature on handshake hash (S2-P09) |
| Implementation crate | `snow = "=0.10.0"` — **pinned exact** (Stage 2 Pin Policy; snow API breaks between majors) |
| Ephemeral key source | HKDF-derived from `keystore.long_term_seed`, `info = b"noise_ephemeral_v1"`, counter = node-start counter (determinism, V-21 extension) |
| Identity binding | `node_id = blake3(ed25519_pubkey)`; Ed25519 signature over noise handshake hash exchanged in handshake payload (S2-P09) |
| MITM detection | Post-handshake, remote static key (noise DH) + Ed25519 node_id signature verified against expected node_id from peer store / hard-coded seed; mismatch → drop connection |

### Handshake state machine sketch (XX pattern)

```
Initiator                                              Responder
─────────                                              ─────────
e (ephemeral pub)  ─────────────────────────────────▶
                              e, ee, s, es
            ◀─────────────────────────────────  e, s (static pub + encrypted)
                              s, se
e, s (identity payload)  ─────────────────────────▶
                              ── transport ready ──
```

| Round | Direction | Message contents | Noise tokens |
|---|---|---|---|
| 1 | I → R | `e` (ephemeral public key) | `e` |
| 2 | R → I | `e, s, es` — responder ephemeral + responder static (encrypted) | `e, ee, s, es` |
| 3 | I → R | `s` (initiator static, encrypted) + identity payload (Ed25519 sig over handshake hash) | `s, se` |

After round 3 both sides call `into_transport_mode()` → `snow::TransportState`
→ all subsequent frames are ChaCha20-Poly1305 AEAD with monotonic nonces
(S2-P08). The Ed25519 identity signature rides in the round-3 payload and is
verified after `into_transport_mode()` against the expected `node_id` (S2-P09).

### Determinism requirements

Per Stage 2 Determinism Checklist (threat V-21 extension):

1. No `SystemTime::now()` / `Instant::now()` in the handshake hot path.
2. No `OsRng` in the handshake — ephemeral keys derived from
   `keystore.long_term_seed` via HKDF (see table above). `OsRng` is permitted
   only for one-time keypair generation at node first start (offline, before
   the network loop).
3. Same seed → same ephemeral key sequence across restarts (forward secrecy
   preserved: key sequence rotates per node start, not per session).
4. Handshake state machine transition (`HandshakeState` → `TransportState`)
   must occur on a single thread (snow API is not thread-safe for this
   transition — see Stage 2 Pitfalls).

## Consequences

### Positive

- **Forward secrecy** — ephemeral X25519 keys per handshake mean a stolen
  long-term static key decrypts no past sessions.
- **Mutual auth without PKI** — both sides prove possession of their static
  key inside the noise handshake; no certificates, no CA.
- **MITM detection** — an attacker who substitutes their own noise keys
  completes the handshake but fails the Ed25519 node_id binding check (S2-P09);
  connection dropped before any consensus data flows.
- **Battle-tested implementation** — `snow` is the de-facto Rust noise
  implementation (used by libp2p-noise, iroh, many others); we avoid
  hand-written crypto (ARCHITECT3 §15 anti-goal).
- **Simple transport API** — `TransportState::encrypt/decrypt` gives a clean
  frame-level API for S2-P08 to wrap the existing length-prefixed framing.

### Negative

- **Pinned dependency** — `snow = "=0.10.0"` exact pin; upgrading requires
  review (Stage 2 Pin Policy). snow 0.9 → 0.10 was breaking; future majors
  likely will be too.
- **Handshake latency** — XX pattern = 3 round trips before the first
  encrypted application frame (~1.5× raw TCP connect-to-first-byte). Acceptable
  for long-lived P2P sessions; reconnect storms mitigated by connection pool
  (S2-P12).
- **Single-thread handshake** — `HandshakeState` → `TransportState` transition
  is not thread-safe in snow; the handshake must run on one thread before the
  transport is shared. Requires explicit ownership transfer (Stage 2 Pitfalls).

### Neutral

- **Ed25519 for internal node identity only** — EOA / transaction signatures
  remain secp256k1 (ADR-0001). The two key types live in segregated keystore
  paths (`keystore/node/` vs `keystore/wallet/`). Operators must not confuse
  them.
- **X25519 vs Ed25519 key separation** — noise DH uses X25519 static keys;
  the Ed25519 key is an *additional* identity binding, not the noise DH key
  itself. Two keypairs per node (noise-static X25519 + identity Ed25519).
- **network_id / proto_version still checked in HELLO (S2-P10)** — noise
  provides the encrypted channel; application-level compatibility negotiation
  (version, network_id, capabilities) stays in the HELLO message, now
  encrypted.

## Alternatives Considered

### Alternative 1: TLS 1.3

- Pros: industry standard, wide tooling, mutual auth via client certs.
- Cons: requires certificate PKI (X.509) or raw self-signed certs with
  ad-hoc pinning; heavyweight for P2P; not used in Bitcoin/Ethereum P2P
  stacks; rustls + x509 parsing pulls a large dependency tree.
- Why not chosen: PKI overhead is unjustified for a permissionless P2P
  network; the static-key binding we need is what noise's XX pattern does
  natively without certificates.

### Alternative 2: Raw TCP without encryption (status quo)

- Pros: zero overhead, zero latency, simplest possible code.
- Cons: threat V-24 (Network MITM) fully unmitigated — passive sniffing of
  txs/blocks, active message injection, peer impersonation.
- Why not chosen: Stage 2 exists precisely to close V-24; ARCHITECT3 §3.6
  explicitly requires noise encryption.

### Alternative 3: Self-implemented crypto (NaCl/TweetNaCl port, custom framing)

- Pros: full control, no external crate pinning.
- Cons: ARCHITECT3 §15 anti-goal («Hand-written cryptographic primitives»);
  high audit burden; subtle nonce/reuse bugs; no handshake state machine —
  would need to invent one (reinventing noise poorly).
- Why not chosen: anti-goal; risk far outweighs the dependency-management
  convenience.

### Alternative 4: NaCl box (libsodium via `sodiumoxide` / `rust_sodium`)

- Pros: simple seal/open API, well-understood.
- Cons: no handshake state machine — static-key exchange and forward secrecy
  must be composed manually; no forward secrecy by default (box uses long-term
  keys directly); libsodium FFI dependency.
- Why not chosen: noise XX gives forward secrecy + mutual auth + a
  standardized state machine in one protocol; NaCl box would require bolting
  on exactly the pieces noise already provides.

### Alternative 5: PSK-based noise (Noise_PSK or Noise_NN with pre-shared key)

- Pros: 1–2 round trips instead of 3; simpler state machine.
- Cons: requires pre-distributing a PSK to all peer pairs — operationally
  impossible in a permissionless network; a single compromised PSK decrypts
  all sessions (no forward secrecy across peers).
- Why not chosen: XX pattern is the only noise pattern providing mutual auth
  without prior key exchange — exactly the P2P threat model.

## Related

- ARCHITECT3 §3.6 (network/p2p), §3.8 (Ed25519 internal auth), §6 V-24
  (Network MITM), §15 (anti-goals), §10.4 (strangler Stage 2)
- ROADMAP3 Этап 2 — «Noise Protocol Framework» (DoD: MITM обнаружен)
- ADR-0001 (secp256k1 for EOA — Ed25519 here is internal-only)
- ADR-0011 (async migration — noise handshake runs inside tokio workers)
- Stage 2 prompts: S2-P05 (crate scaffolding, pins `snow`), S2-P07 (handshake
  state machine), S2-P08 (transport framing), S2-P09 (Ed25519 node_id auth)
- Stage 2 Pin Policy (AGENTS.md) — `snow = "=0.10.0"` rationale
- Stage 2 Determinism Checklist (AGENTS.md) — ephemeral key derivation

## Renumbering note

ROADMAP3 originally planned this ADR as ADR-0016. Stage 1.5 FBP07 occupied
ADR-0016 for «Receipts Root in Block Header» (commit `768fc99`, superseding
any planned reuse). Stage 2 ADRs are therefore renumbered to ADR-0020..0023
(noise / erlay / peer-scoring / rate-limiting) to avoid a breaking overwrite
of an ADR already referenced by consensus code (`BlockHeader.receipts_root`).
This decision is recorded in the S2-P01 commit message.
