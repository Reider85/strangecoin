//! S3-P10 crash-recovery suite: kill during writes → clean reopen.
//!
//! Covers two ROADMAP3/ADR-0024 obligations the S3-P06 crash *smoke*
//! explicitly deferred (it documented `abort()` ≠ real kill and pointed
//! here):
//!
//! 1. **Atomic batches under `Child::kill`** — a child process writes
//!    identifiable batches and is force-terminated (`TerminateProcess` on
//!    Windows / SIGKILL-class on Unix) at a chosen point; the parent
//!    reopens the same directory with *no manual intervention* and finds
//!    every batch either fully present or fully absent, with the tip
//!    height consistent with the highest complete batch.
//! 2. **Stale LOCK auto-release** — after the child dies holding the DB
//!    open, the OS releases RocksDB's LOCK file; reopen succeeds without
//!    anyone deleting the lock manually (the LevelDB-era failure mode
//!    this suite exists to prove gone).
//!
//! *Deviation (FBP, S3-P10): the prompt said "random точка". Kill points
//! are a deterministic sweep (`KILL_AFTER ∈ {0,1,5,50}`) instead — the
//! Stage 3 determinism checklist forbids RNG on storage paths, and a
//! sweep both reproduces and covers more of the write timeline than one
//! random draw.*
//!
//! Limitation (same as S3-P06): kill ≠ power loss. With `sync = false`
//! the WAL bytes already sit in the OS page cache when `write_batch`
//! returns; power-loss durability is the operator `flush()` barrier
//! (ADR-0024 §Write contract), not this suite.
//!
//! The crate builds without the `rocksdb` feature too: this file compiles
//! to nothing then.
#![cfg(feature = "rocksdb")]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use strangecoin_storage::{
    encode_schema_version, meta_key, Cf, RocksDbEngine, StorageEngine, META_SCHEMA_VERSION,
    META_TIP_HEIGHT, SCHEMA_VERSION,
};

/// Env marker selecting the child leg (same pattern as the S3-P06 smoke).
const CHILD_ENV: &str = "STRANGECOIN_S3_P10_CRASH_CHILD";
/// Child writes this many batches *before* signalling the parent to kill.
const KILL_AFTER_ENV: &str = "STRANGECOIN_S3_P10_KILL_AFTER";
/// Child mode: hold the DB open and wait to be killed (LOCK test).
const LOCK_MODE_ENV: &str = "STRANGECOIN_S3_P10_LOCK_MODE";

/// Puts per batch — a partial batch is observable if atomicity breaks.
const BATCH_KEYS: usize = 10;

fn batch_key(batch: u64, j: usize) -> Vec<u8> {
    format!("batch_{batch}_{j:02}").into_bytes()
}

fn batch_value(batch: u64) -> Vec<u8> {
    format!("v{batch}").into_bytes()
}

// ---------------------------------------------------------------------------
// Child legs
// ---------------------------------------------------------------------------

/// Crash child: write batches `0..` printing `progress <i>` after each
/// completed `write_batch`; after `kill_after` batches the parent is
/// expected to terminate us mid-loop (often while batch `kill_after+1`
/// is in flight — that is the point).
fn run_crash_child(dir: &str, kill_after: u64) -> ! {
    let engine = RocksDbEngine::open(dir).expect("child open");
    let mut i: u64 = 0;
    loop {
        let mut ops: Vec<strangecoin_storage::WriteBatchOp> = (0..BATCH_KEYS)
            .map(|j| strangecoin_storage::WriteBatchOp::Put {
                cf: Cf::Mempool,
                key: batch_key(i, j),
                value: batch_value(i),
            })
            .collect();
        // Tip rides the same batch: atomicity means tip can never point
        // at a batch whose keys are only half on disk.
        ops.push(strangecoin_storage::WriteBatchOp::Put {
            cf: Cf::Meta,
            key: meta_key(META_TIP_HEIGHT),
            value: i.to_be_bytes().to_vec(),
        });
        engine.write_batch(ops).expect("child write_batch");

        println!("progress {i}");
        // Line-buffered stdout so the parent sees the marker immediately.
        use std::io::Write;
        std::io::stdout().flush().expect("child stdout flush");

        if i >= kill_after {
            // Keep writing so the parent's kill lands during active work.
            std::thread::sleep(Duration::from_millis(50));
        }
        i = i.wrapping_add(1);
    }
}

/// LOCK child: open the DB (creating/holding the OS LOCK), signal, sleep
/// until the parent kills us.
fn run_lock_child(dir: &str) -> ! {
    let _engine = RocksDbEngine::open(dir).expect("lock child open");
    println!("ready");
    use std::io::Write;
    std::io::stdout().flush().expect("child stdout flush");
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

// ---------------------------------------------------------------------------
// Parent helpers
// ---------------------------------------------------------------------------

fn spawn_crash_child(test_name: &str, dir: &std::path::Path, kill_after: u64) -> Child {
    let exe = std::env::current_exe().expect("test binary path");
    Command::new(&exe)
        .arg(test_name)
        .arg("--exact")
        .arg("--nocapture")
        .env(CHILD_ENV, dir)
        .env(KILL_AFTER_ENV, kill_after.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn crash child process")
}

/// Read child stdout lines until `progress <kill_after>` appears.
fn wait_for_progress(child: &mut Child, kill_after: u64) {
    let stdout = child.stdout.take().expect("child stdout piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "child never reached progress {kill_after}"
        );
        line.clear();
        let n = reader
            .read_line(&mut line)
            .expect("read child stdout");
        assert!(n > 0, "child stdout closed before progress {kill_after}");
        if let Some(rest) = line.trim().strip_prefix("progress ") {
            let seen: u64 = rest.parse().expect("progress number");
            if seen >= kill_after {
                // Put the buffered remainder back is unnecessary: the
                // child keeps writing; we only needed the sync point.
                return;
            }
        }
    }
}

fn wait_for_ready(child: &mut Child) {
    let stdout = child.stdout.take().expect("child stdout piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    // The harness may emit "running 1 test" before the test body —
    // skip lines until the child's own "ready" marker arrives.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "lock child never signalled ready"
        );
        line.clear();
        let n = reader.read_line(&mut line).expect("read child stdout");
        if n == 0 {
            // EOF: the child died before ready — surface its exit status
            // (stderr is inherited, so any panic already printed above).
            let status = child.wait().expect("wait dead child");
            panic!("lock child exited before ready: {status:?}");
        }
        if line.trim() == "ready" {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Parent half of a crash test: spawn `test_name` as a child (it will
/// take the child leg via the env markers), kill it after `kill_after`
/// completed batches, reopen, assert atomicity + tip consistency.
fn crash_then_reopen_is_atomic(test_name: &str, kill_after: u64) {
    // Child leg: this very test re-spawns itself with the env markers.
    if let Ok(child_dir) = std::env::var(CHILD_ENV) {
        let kill_after: u64 = std::env::var(KILL_AFTER_ENV)
            .expect("child must set KILL_AFTER")
            .parse()
            .expect("kill_after is a number");
        run_crash_child(&child_dir, kill_after);
    }

    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn_crash_child(test_name, dir.path(), kill_after);
    wait_for_progress(&mut child, kill_after);

    // Force-terminate while the child is (likely) mid-write of a later
    // batch. TerminateProcess / SIGKILL — no destructor, no flush.
    // (Kill may race a broken-pipe exit after stdout closes — either
    // death is an abrupt process death for our purposes.)
    let _ = child.kill();
    let _ = child.wait();

    // Reopen with zero manual cleanup (no LOCK removal, no repair).
    let engine =
        RocksDbEngine::open(dir.path()).expect("reopen after kill must succeed without intervention");

    // Schema guard still holds after the crash.
    let version = engine
        .get(Cf::Meta, &meta_key(META_SCHEMA_VERSION))
        .expect("schema version read")
        .expect("schema version present");
    assert_eq!(version, encode_schema_version(SCHEMA_VERSION).to_vec());

    // Tip must be a complete batch: read it, then verify every key of
    // that batch and the all-or-none property for every batch index.
    let tip_bytes = engine
        .get(Cf::Meta, &meta_key(META_TIP_HEIGHT))
        .expect("tip read")
        .expect("tip must exist (child wrote batch 0 before any kill point)");
    assert_eq!(tip_bytes.len(), 8, "tip height is a u64 BE");
    let mut tip = [0u8; 8];
    tip.copy_from_slice(&tip_bytes);
    let tip = u64::from_be_bytes(tip);

    // Scan every batch index the child could have started: from 0 up to
    // a bound past kill_after (child keeps writing after the marker, but
    // the kill happens promptly — kill_after + 64 is a safe ceiling for
    // the 50 ms cadence). All-or-none per index; tip = max complete.
    let ceiling = kill_after.saturating_add(64);
    let mut max_complete = None;
    for i in 0..=ceiling {
        let present: usize = (0..BATCH_KEYS)
            .filter(|j| {
                engine
                    .get(Cf::Mempool, &batch_key(i, *j))
                    .expect("batch key read")
                    .is_some()
            })
            .count();
        assert!(
            present == 0 || present == BATCH_KEYS,
            "batch {i} partially present ({present}/{BATCH_KEYS}) — atomicity violated"
        );
        if present == BATCH_KEYS {
            // Value must be the batch's own (no torn/merged values).
            for j in 0..BATCH_KEYS {
                let v = engine
                    .get(Cf::Mempool, &batch_key(i, j))
                    .expect("read")
                    .expect("present");
                assert_eq!(v, batch_value(i), "batch {i} key {j} has wrong value");
            }
            max_complete = Some(i);
        }
    }
    let max_complete = max_complete.expect("at least batch 0 completed before kill");
    assert!(
        tip <= max_complete,
        "tip {tip} points past the last complete batch {max_complete}"
    );
    assert!(
        tip >= kill_after.min(max_complete),
        "tip {tip} must cover the batches the child reported complete ({kill_after})"
    );
    // Tip is a Put inside the same WriteBatch as the batch keys, so a
    // complete batch implies its tip value landed — tip tracks the last
    // complete batch exactly.
    assert_eq!(
        tip, max_complete,
        "tip must equal the last fully-present batch (atomicity of tip+keys)"
    );
}

#[test]
fn crash_kill_after_0_reopens_atomically() {
    crash_then_reopen_is_atomic("crash_kill_after_0_reopens_atomically", 0);
}

#[test]
fn crash_kill_after_1_reopens_atomically() {
    crash_then_reopen_is_atomic("crash_kill_after_1_reopens_atomically", 1);
}

#[test]
fn crash_kill_after_5_reopens_atomically() {
    crash_then_reopen_is_atomic("crash_kill_after_5_reopens_atomically", 5);
}

#[test]
fn crash_kill_after_50_reopens_atomically() {
    crash_then_reopen_is_atomic("crash_kill_after_50_reopens_atomically", 50);
}

/// Stale LOCK: kill -9-class death while the child holds the DB open;
/// the parent reopens the same directory immediately — success **without**
/// manually deleting the LOCK file (the pre-RocksDB LevelDB failure mode).
#[test]
fn stale_lock_is_auto_released_after_child_kill() {
    const TEST: &str = "stale_lock_is_auto_released_after_child_kill";

    // Child leg (this process was re-spawned with the env markers).
    if let Ok(dir) = std::env::var(CHILD_ENV) {
        if std::env::var(LOCK_MODE_ENV).is_ok() {
            run_lock_child(&dir);
        }
        // Fall through only for misconfigured env — treat as failure.
        panic!("crash child started without lock mode in this test");
    }

    let dir = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().expect("test binary path");
    let mut child = Command::new(&exe)
        .arg(TEST)
        .arg("--exact")
        .arg("--nocapture")
        .env(CHILD_ENV, dir.path())
        .env(LOCK_MODE_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn lock child");
    wait_for_ready(&mut child);

    // The child holds RocksDB's LOCK right now. Kill it hard; the OS
    // must release the lock with the process — no cleanup by us.
    child.kill().expect("kill lock child");
    let _ = child.wait();

    // No `fs::remove_file(LOCK)`, no sleep-retry loop: immediate reopen.
    let engine = RocksDbEngine::open(dir.path())
        .expect("reopen must succeed after kill without manual LOCK removal");
    // And the reopened engine is fully usable.
    engine
        .put(Cf::Meta, b"post_lock_key", b"ok")
        .expect("write after lock takeover");
    engine.flush().expect("flush after lock takeover");
    assert_eq!(
        engine.get(Cf::Meta, b"post_lock_key").unwrap(),
        Some(b"ok".to_vec())
    );
}
