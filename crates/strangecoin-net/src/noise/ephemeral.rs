//! S2-P09 — deterministic Noise ephemeral keys (Stage 2 determinism
//! checklist #2/#3, threat V-21 extension).
//!
//! `snow`'s default resolver draws the handshake ephemeral keypair from the
//! OS CSPRNG at handshake time — exactly what the checklist forbids
//! ("no `thread_rng()` / `OsRng` in handshake"). Instead every handshake
//! builds its `snow::Builder` with [`SeedResolver`], which derives the random
//! source from the node's long-term seed:
//!
//! ```text
//! ephemeral_seed = HKDF-SHA256(salt = None,
//!                              ikm = long_term_node_seed,
//!                              info = b"noise_ephemeral_v1" || counter_be)
//! ```
//!
//! `counter` advances once per handshake on the [`super::NodeKeypair`]
//! (shared through its `Arc`), so:
//!
//! * **fresh per handshake** — every session in a node's run gets its own
//!   ephemeral key (forward secrecy against ephemeral-only compromise), and
//! * **reproducible** — the same seed replays the same sequence, which is what
//!   `cargo test --features deterministic_replay` and the shadow-network tooling
//!   need (AGENTS.md §Stage 2 Determinism Checklist; the S2-P07/S2-P08
//!   deferred obligation closes here).
//!
//! All *other* primitives (DH, hash, cipher) delegate to snow's
//! [`DefaultResolver`] — only the entropy source is replaced, the crypto
//! suite itself stays exactly what ADR-0020 pinned.

use hkdf::Hkdf;
use sha2::Sha256;
use snow::params::{CipherChoice, DHChoice, HashChoice};
use snow::resolvers::{CryptoResolver, DefaultResolver};
use snow::types::{Cipher, Dh, Hash, Random};

/// HKDF `info` domain separator for Noise ephemeral seeds.
const EPHEMERAL_INFO: &[u8] = b"noise_ephemeral_v1";

/// HKDF `info` domain separator for the long-term Noise X25519 static key
/// (S2-P12 — the DH identity `NoiseHandshake::initiator/responder` takes as
/// `static_key`). Distinct from [`EPHEMERAL_INFO`] so the two derivations can
/// never collide on the same seed.
const NOISE_STATIC_INFO: &[u8] = b"noise_static_v1";

/// Derive the per-handshake ephemeral seed from the node's long-term seed
/// and the keypair's handshake counter (Stage 2 checklist #3).
pub(crate) fn ephemeral_seed(long_term_seed: &[u8; 32], counter: u64) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, long_term_seed);
    let mut info = [0u8; EPHEMERAL_INFO.len() + 8];
    info[..EPHEMERAL_INFO.len()].copy_from_slice(EPHEMERAL_INFO);
    info[EPHEMERAL_INFO.len()..].copy_from_slice(&counter.to_be_bytes());
    let mut okm = [0u8; 32];
    hk.expand(&info, &mut okm)
        .expect("HKDF-SHA256 expand to 32 bytes stays below the 255*32 limit");
    okm
}

/// Derive the node's long-term Noise X25519 static private key from the
/// keystore seed (S2-P12 production sourcing for `connect_to_peer`).
///
/// Deterministic (Stage 2 checklist #2/#3): the same long-term seed yields
/// the same static key on every start, no OS RNG and no extra keystore entry.
/// Forward secrecy is unaffected — it comes from the per-handshake ephemeral
/// keys ([`ephemeral_seed`]), not from this static DH identity.
pub(crate) fn noise_static_seed(long_term_seed: &[u8; 32]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, long_term_seed);
    let mut okm = [0u8; 32];
    hk.expand(NOISE_STATIC_INFO, &mut okm)
        .expect("HKDF-SHA256 expand to 32 bytes stays below the 255*32 limit");
    okm
}

/// A snow [`CryptoResolver`] that replaces **only** the RNG: everything else
/// falls through to [`DefaultResolver`], so the negotiated Noise suite
/// (`Noise_XX_25519_ChaChaPoly_BLAKE2s`) is untouched.
pub(crate) struct SeedResolver {
    seed: [u8; 32],
}

impl SeedResolver {
    pub(crate) fn new(seed: [u8; 32]) -> Self {
        Self { seed }
    }
}

impl CryptoResolver for SeedResolver {
    fn resolve_rng(&self) -> Option<Box<dyn Random>> {
        Some(Box::new(Blake3Random::new(self.seed)))
    }

    fn resolve_dh(&self, choice: &DHChoice) -> Option<Box<dyn Dh>> {
        DefaultResolver.resolve_dh(choice)
    }

    fn resolve_hash(&self, choice: &HashChoice) -> Option<Box<dyn Hash>> {
        DefaultResolver.resolve_hash(choice)
    }

    fn resolve_cipher(&self, choice: &CipherChoice) -> Option<Box<dyn Cipher>> {
        DefaultResolver.resolve_cipher(choice)
    }
}

/// Deterministic stream: blake3 in counter (PRF) mode over the derived
/// ephemeral seed. A PRF keystream is a standard CSPRNG construction; here it
/// only ever feeds snow's ephemeral key generation, which needs 32 bytes once
/// per handshake.
struct Blake3Random {
    seed: [u8; 32],
    block_index: u64,
    block: [u8; 32],
    pos: usize,
}

impl Blake3Random {
    fn new(seed: [u8; 32]) -> Self {
        Self {
            seed,
            block_index: 0,
            block: [0u8; 32],
            pos: 32, // force a refill on the first pull
        }
    }

    fn refill(&mut self) {
        self.block = *blake3::keyed_hash(&self.seed, &self.block_index.to_be_bytes()).as_bytes();
        self.block_index = self.block_index.wrapping_add(1);
        self.pos = 0;
    }
}

impl Random for Blake3Random {
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), snow::Error> {
        for byte in dest {
            if self.pos == self.block.len() {
                self.refill();
            }
            *byte = self.block[self.pos];
            self.pos += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_static_key_is_deterministic_per_seed_and_distinct_from_ephemeral() {
        assert_eq!(noise_static_seed(&[7u8; 32]), noise_static_seed(&[7u8; 32]));
        assert_ne!(noise_static_seed(&[7u8; 32]), noise_static_seed(&[8u8; 32]));
        // Different HKDF info domains: the static key never equals any
        // ephemeral seed drawn from the same long-term seed.
        assert_ne!(noise_static_seed(&[7u8; 32]), ephemeral_seed(&[7u8; 32], 0));
    }

    #[test]
    fn same_seed_and_counter_derive_the_same_ephemeral_seed() {
        assert_eq!(ephemeral_seed(&[7u8; 32], 0), ephemeral_seed(&[7u8; 32], 0));
        assert_ne!(ephemeral_seed(&[7u8; 32], 0), ephemeral_seed(&[7u8; 32], 1));
        assert_ne!(ephemeral_seed(&[7u8; 32], 1), ephemeral_seed(&[8u8; 32], 1));
    }

    #[test]
    fn blake3_random_is_reproducible_and_covers_arbitrary_lengths() {
        let mut a = Blake3Random::new([9u8; 32]);
        let mut b = Blake3Random::new([9u8; 32]);

        // Longer than one blake3 block → exercises block rollover.
        let mut buf_a = [0u8; 100];
        let mut buf_b = [0u8; 100];
        a.try_fill_bytes(&mut buf_a).expect("fill a");
        b.try_fill_bytes(&mut buf_b).expect("fill b");
        assert_eq!(buf_a, buf_b, "same seed must replay the same stream");

        let mut again = [0u8; 100];
        a.try_fill_bytes(&mut again).expect("fill again");
        assert_ne!(buf_a, again, "the stream must advance between pulls");
    }

    #[test]
    fn seed_resolver_delegates_primitives_to_the_default_resolver() {
        use snow::params::{CipherChoice, DHChoice, HashChoice};

        let resolver = SeedResolver::new([1u8; 32]);
        assert!(resolver.resolve_dh(&DHChoice::Curve25519).is_some());
        assert!(resolver.resolve_hash(&HashChoice::Blake2s).is_some());
        assert!(resolver.resolve_cipher(&CipherChoice::ChaChaPoly).is_some());
        assert!(resolver.resolve_rng().is_some());
    }
}
