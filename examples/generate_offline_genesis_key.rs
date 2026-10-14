//! BUG-S1-015 / SCIP-0001 §4 — offline genesis key generation tooling.
//!
//! Generates a fresh secp256k1 keypair and prints **only** the compressed
//! public key (33 bytes, `0x`-prefixed hex) to stdout, suitable for
//! `genesis.json:initial_holder_pubkey`. The private key never goes to
//! stdout; it is written to an optional file argument (refusing to
//! overwrite) or, if no path is given, printed to stderr with a warning.
//!
//! **Security contract (SCIP-0001 §1):**
//! - Run on an **air-gapped machine** when producing a mainnet key.
//! - The private key is stored offline (paper/HSM); it is **never** committed
//!   to the repository, never embedded in binaries, never logged.
//! - Only the public key from stdout may enter `genesis.json`.
//!
//! Run:
//! ```text
//! cargo run --release --example generate_offline_genesis_key > genesis_pubkey.txt
//! # optional: also persist the private key to a file (refuses overwrite)
//! cargo run --release --example generate_offline_genesis_key -- genesis_privkey.hex
//! ```

use rand::rngs::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use std::fs::OpenOptions;
use std::io::Write;
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "Usage: cargo run --release --example generate_offline_genesis_key -- [privkey_out_path]\n\
             \n\
             stdout: 0x-prefixed 33-byte compressed secp256k1 pubkey (one line)\n\
             privkey: hex written to privkey_out_path (refuses overwrite), or stderr if omitted\n\
             \n\
             Run on an AIR-GAPPED machine for mainnet keys (SCIP-0001 §4)."
        );
        exit(0);
    }
    if args.len() > 1 {
        eprintln!("error: expected at most one argument (privkey output path)");
        exit(1);
    }

    let secp = Secp256k1::new();
    let secret_key = SecretKey::new(&mut OsRng);
    let public_key = PublicKey::from_secret_key(&secp, &secret_key);

    // stdout: public key only — machine-readable, safe to redirect.
    println!("0x{}", hex::encode(public_key.serialize()));

    // Private key: file (preferred) or stderr — never stdout.
    let privkey_hex = hex::encode(secret_key.secret_bytes());
    match args.first() {
        Some(path) => {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path);
            let mut file = match file {
                Ok(f) => f,
                Err(e) => {
                    eprintln!(
                        "error: cannot create private key file `{path}`: {e}\n\
                         (refusing to overwrite an existing file — delete it deliberately if intended)"
                    );
                    exit(1);
                }
            };
            if let Err(e) = writeln!(file, "{privkey_hex}") {
                eprintln!("error: failed to write private key to `{path}`: {e}");
                exit(1);
            }
            eprintln!(
                "[warn] private key written to `{path}`.\n\
                 [warn] Move it to offline vault media (paper/HSM) and delete the local copy.\n\
                 [warn] NEVER commit this file or paste the key into any networked system."
            );
        }
        None => {
            eprintln!("[warn] private key (write to offline vault, then erase):");
            eprintln!("{privkey_hex}");
            eprintln!(
                "[warn] This key controls the genesis allocation. Anyone with it can drain it.\n\
                 [warn] Generate mainnet keys only on an AIR-GAPPED machine (SCIP-0001 §4)."
            );
        }
    }

    eprintln!(
        "[info] Next steps: paste the stdout pubkey into genesis.json:initial_holder_pubkey,\n\
         [info] run `cargo run -- --print-genesis-hash`, update EXPECTED_GENESIS_HASH,\n\
         [info] and run `cargo test --test genesis_key` (SCIP-0001 §3)."
    );
}
