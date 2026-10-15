//! One-off helper used by S1.5-P08 to regenerate EXPECTED_*_GENESIS_HASH
//! after the FORMAT_VERSION 5 transaction encoding bump. Prints both network
//! genesis hashes in the constant-literal form.

fn main() {
    for (path, label) in [
        ("genesis.json", "mainnet"),
        ("genesis-testnet.json", "testnet"),
    ] {
        let block = strangecoin::consensus::load_genesis(path)
            .unwrap_or_else(|e| panic!("load {path}: {e}"));
        let hash = strangecoin_core::serialize::block_hash(&block);
        println!("// {label} ({path})");
        println!("pub const HASH: [u8; 32] = [");
        for chunk in hash.chunks(12) {
            let line: Vec<String> = chunk.iter().map(|b| format!("0x{:02x},", b)).collect();
            println!("    {}", line.join(" "));
        }
        println!("];");
        println!("// hex: 0x{}", hex::encode(hash));
        println!();
    }
}
