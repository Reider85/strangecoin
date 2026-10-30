//! Key encoding + schema versioning for the ADR-0024 column families.
//!
//! Pure byte functions only — no I/O, no clocks, no RNG (Stage 3
//! determinism checklist). This module intentionally does **not** use
//! `strangecoin-core` types: it encodes key *structure* (heights, hashes,
//! addresses, slots, nonces); value codecs for `Block`/`AccountState`/
//! `Transaction` stay at the monolith edge (ADR-0024 §Decision "Values").
//!
//! Crate-level dependency direction: `strangecoin-storage` →
//! `strangecoin-core` is allowed for snapshot (S3-P13) / witness (S3-P11)
//! modules; the reverse is forbidden (ADR-0024 §Decision "Values").
//!
//! Implemented in S3-P05: `KeyPrefix` constructors (big-endian `u64`
//! heights/nonces, lexicographic order == numeric order), `SCHEMA_VERSION`,
//! `meta.schema_version` encode/decode.
