//! S3-P10 microbench: batch validation (WriteBatch) vs sequential puts.
//!
//! ROADMAP3 Этап 3 DoD #6: «Пакетная валидация ≥10x быстрее sequential».
//! The КГ asks for a measured ratio fixed in the commit; the ROADMAP
//! threshold is checked against the measurement, and a miss would be
//! recorded as an FBP deviation with the data rather than a silent pass.
//!
//! *Deviation (FBP, S3-P10): the prompt offered `benches/write_batch.rs`
//! under criterion **or** a test-based harness with an explicit deviation
//! note. Criterion is not present anywhere in this workspace (adding it
//! would grow `clippy --all-targets` by a full bench dep-tree for one
//! measurement), and Stage 2 already established Instant-based timing
//! tests as the local-precedent — this file is the test-based variant.*
//!
//! *Determinism (Stage 3 checklist #2): the "random" account-upsert keys
//! come from a fixed-seed LCG, never `thread_rng` — the measured ratio is
//! reproducible run-to-run on the same machine (absolute times still
//! fluctuate with load; only the order-of-magnitude claim is asserted).*
//!
//! The crate builds without the `rocksdb` feature too: this file compiles
//! to nothing then.
#![cfg(feature = "rocksdb")]

use std::time::Instant;

use strangecoin_storage::{state_key, Cf, RocksDbEngine, StorageEngine, WriteBatchOp};

/// Number of account-upserts per arm (prompt task 1: 1000).
const N: usize = 1000;

/// Deterministic LCG (Numerical Recipes constants) — fixed seed so the
/// key sequence (and therefore the workload) is identical every run.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// N distinct account addresses via fixed-seed LCG (bech32-shaped, the
/// schema's fixed-length address invariant is a monolith-edge concern —
/// the engine stores opaque bytes, any UTF-8 address works here).
fn account_keys(n: usize) -> Vec<Vec<u8>> {
    let mut lcg = Lcg::new(0x53_43_54_50_31_30); // "SCTP10"
    let mut keys = Vec::with_capacity(n);
    let mut seen = std::collections::HashSet::new();
    while keys.len() < n {
        let addr = format!("sc1{:032x}", lcg.next() ^ (keys.len() as u64).wrapping_mul(0x9E37));
        if seen.insert(addr.clone()) {
            keys.push(state_key(&addr));
        }
    }
    keys
}

/// Deterministic upsert payload (value depends only on the index).
fn account_value(i: usize) -> Vec<u8> {
    format!("acct-v{i:08}").into_bytes()
}

#[test]
fn batch_write_is_faster_than_sequential_put_flush() {
    let dir = tempfile::tempdir().unwrap();
    let keys = account_keys(N);

    // --- arm (a): per-op put + flush (1000 durability barriers) -------
    let a = {
        let engine = RocksDbEngine::open(dir.path().join("seq")).expect("open seq db");
        let start = Instant::now();
        for (i, key) in keys.iter().enumerate() {
            engine
                .put(Cf::State, key, &account_value(i))
                .expect("sequential put");
            engine.flush().expect("sequential flush");
        }
        start.elapsed()
    };

    // --- arm (b): one write_batch + one flush (single barrier) --------
    let b = {
        let engine = RocksDbEngine::open(dir.path().join("batch")).expect("open batch db");
        let ops: Vec<WriteBatchOp> = keys
            .iter()
            .enumerate()
            .map(|(i, key)| WriteBatchOp::Put {
                cf: Cf::State,
                key: key.clone(),
                value: account_value(i),
            })
            .collect();
        let start = Instant::now();
        engine.write_batch(ops).expect("batch write");
        engine.flush().expect("batch flush");
        start.elapsed()
    };

    // Correctness both arms wrote the same N records (guards against a
    // broken harness reporting a vacuous ratio).
    let seq = RocksDbEngine::open(dir.path().join("seq")).expect("reopen seq");
    let bat = RocksDbEngine::open(dir.path().join("batch")).expect("reopen batch");
    assert_eq!(
        seq.iterate_prefix(Cf::State, b"").unwrap().len(),
        N,
        "sequential arm must persist all N upserts"
    );
    assert_eq!(
        bat.iterate_prefix(Cf::State, b"").unwrap().len(),
        N,
        "batch arm must persist all N upserts"
    );

    // Soft assert per prompt: batch strictly faster. The ROADMAP ≥10x
    // claim is measured and logged; 1000 flushes vs 1 makes a miss
    // practically impossible, but the hard gate is ordering only.
    let ratio = a.as_secs_f64() / b.as_secs_f64();
    println!(
        "write_batch bench: sequential(put+flush)x{N} = {a:?}, \
         one write_batch+flush = {b:?}, ratio = {ratio:.1}x"
    );
    assert!(
        b < a,
        "one write_batch must beat N per-op put+flush (a={a:?}, b={b:?})"
    );
    assert!(
        ratio >= 10.0,
        "ROADMAP3 DoD #6 expects ≥10x; measured {ratio:.1}x — \
         record as deviation with data if this ever fails"
    );
}
