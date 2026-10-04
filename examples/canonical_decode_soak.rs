//! S1-P19 fallback fuzz runner: deterministic random-bytes + mutated-real-block
//! soak against the canonical decoders, for hosts where cargo-fuzz/libFuzzer
//! cannot run (see `fuzz/README.md` for the toolchain constraints). Same
//! contract as the libFuzzer target: every input either decodes or returns a
//! typed error — a panic aborts the process and fails the run.
//!
//! Run: `SOAK_SECONDS=600 cargo run --example canonical_decode_soak`

use std::time::{Duration, Instant};

use strangecoin_core::serialize::{
    deserialize_block, deserialize_header, deserialize_transaction,
    deserialize_transaction_signed, serialize_block,
};
use strangecoin_core::types::{Block, Transaction};

/// Xorshift64*: deterministic, dependency-free byte stirring.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// A structurally valid regtest-shaped block — the mutation seed.
fn seed_block() -> Block {
    let tx = Transaction {
        sender: "coinbase".to_string(),
        receiver: "miner".to_string(),
        amount: 0,
        nonce: 0,
        chain_id: 1,
        signature: Vec::new(),
        is_coinbase: true,
    };
    Block {
        index: 1,
        timestamp: 1_700_000_000,
        transactions: vec![tx],
        previous_hash: "11".repeat(32),
        hash: String::new(),
        nonce: 0,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

fn main() {
    let seconds: u64 = std::env::var("SOAK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let mut rng = Xorshift(0x5EED_5EED_5EED_5EED);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let seed = serialize_block(&seed_block());
    let mut execs: u64 = 0;

    while Instant::now() < deadline {
        let buf = match execs % 3 {
            // Sub-frame garbage.
            0 => {
                let len = (rng.next() % 16) as usize;
                (0..len).map(|_| rng.next() as u8).collect::<Vec<u8>>()
            }
            // Random buffers across decoder-relevant sizes.
            1 => {
                let len = match execs % 7 {
                    0 => 173,
                    1 => 320,
                    2 => 1024,
                    _ => (rng.next() % 4096) as usize,
                };
                (0..len).map(|_| rng.next() as u8).collect::<Vec<u8>>()
            }
            // A real serialized block with 1–4 mutated bytes — this is the
            // phase that reaches deep decoder paths.
            2 => {
                let mut buf = seed.clone();
                let flips = 1 + (rng.next() % 4) as usize;
                for _ in 0..flips {
                    let idx = (rng.next() as usize) % buf.len().max(1);
                    if let Some(byte) = buf.get_mut(idx) {
                        *byte ^= (rng.next() as u8) | 1;
                    }
                }
                buf
            }
            _ => unreachable!(),
        };

        let _ = deserialize_header(&buf);
        let _ = deserialize_block(&buf);
        let _ = deserialize_transaction(&buf);
        let _ = deserialize_transaction_signed(&buf);
        execs += 1;
    }

    println!("canonical_decode_soak: {execs} inputs in {seconds}s, 0 panics");
}
