//! Gate: no direct `balances` mutations outside the state-cache layer
//! (BUG-S0-022, S1-P12 КГ).
//!
//! Executable form of the rg audit:
//!
//! ```text
//! rg '\.balances\.(insert|remove|get_mut)' src/ \
//!     --glob '!state_cache.rs' --glob '!block_executor.rs'   # -> 0 matches
//! rg '\.balances\s*=[^=]' src/ \
//!     --glob '!state_cache.rs' --glob '!chain_selector.rs'   # -> 0 matches
//! ```
//!
//! Raw HashMap mutations are confined to `state_cache.rs` (the StateCache
//! write API) and `block_executor.rs` (genesis credit path). Whole-field
//! replacement is confined to `state_cache.rs` (DB load / address migration)
//! and `chain_selector.rs` (reorg adoption of a rebuilt cache).

use std::path::{Path, PathBuf};

const RAW_MUTATION_WHITELIST: [&str; 2] = ["state_cache.rs", "block_executor.rs"];
const FIELD_REPLACEMENT_WHITELIST: [&str; 2] = ["state_cache.rs", "chain_selector.rs"];

const RAW_MUTATION_PATTERNS: [&str; 3] = [
    ".balances.insert",
    ".balances.remove",
    ".balances.get_mut",
];

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("failed to read src/");
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn is_whitelisted(rel_path: &str, whitelist: &[&str; 2]) -> bool {
    whitelist
        .iter()
        .any(|name| rel_path.ends_with(name))
}

fn is_raw_mutation_violation(rel_path: &str, line: &str) -> bool {
    if is_whitelisted(rel_path, &RAW_MUTATION_WHITELIST) {
        return false;
    }
    RAW_MUTATION_PATTERNS
        .iter()
        .any(|pattern| line.contains(pattern))
}

fn is_field_replacement_violation(rel_path: &str, line: &str) -> bool {
    if is_whitelisted(rel_path, &FIELD_REPLACEMENT_WHITELIST) {
        return false;
    }
    let mut search = 0usize;
    while let Some(pos) = line[search..].find(".balances") {
        let after_field = search + pos + ".balances".len();
        let rest = line[after_field..].trim_start();
        if let Some(after_eq) = rest.strip_prefix('=') {
            if !after_eq.starts_with('=') {
                return true;
            }
        }
        search = after_field;
    }
    false
}

#[test]
fn no_direct_balances_mutations_outside_state_cache_layer() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_dir, &mut files);
    assert!(!files.is_empty(), "no .rs files found under src/");

    let mut violations = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(manifest_dir)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let content = std::fs::read_to_string(file).expect("failed to read source file");
        for (idx, line) in content.lines().enumerate() {
            let lineno = idx + 1;
            if is_raw_mutation_violation(&rel, line) {
                violations.push(format!(
                    "{rel}:{lineno}: raw balances mutation: {}",
                    line.trim()
                ));
            }
            if is_field_replacement_violation(&rel, line) {
                violations.push(format!(
                    "{rel}:{lineno}: whole-field balances replacement: {}",
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "direct balances mutations outside the whitelisted files (BUG-S0-022 / S1-P12):\n{}",
        violations.join("\n")
    );
}

#[test]
fn gate_rejects_synthetic_violations_and_accepts_whitelisted_ones() {
    assert!(is_raw_mutation_violation(
        "src/blockchain/facade.rs",
        "guard.balances.insert(addr, state);"
    ));
    assert!(is_raw_mutation_violation(
        "src/lib.rs",
        "bc.balances.get_mut(\"a\").unwrap().balance = 1;"
    ));
    assert!(!is_raw_mutation_violation(
        "src/blockchain/state_cache.rs",
        "self.accounts.insert(addr, state);"
    ));
    assert!(!is_raw_mutation_violation(
        "src/blockchain/blockchain_facade.rs",
        "let total = nonzero_balances_total();"
    ));

    assert!(is_field_replacement_violation(
        "src/blockchain/block_executor.rs",
        "blockchain.balances = rebuilt;"
    ));
    assert!(!is_field_replacement_violation(
        "src/blockchain/state_cache.rs",
        "self.balances = StateCache::from_accounts(new_accounts);"
    ));
    assert!(!is_field_replacement_violation(
        "src/blockchain/chain_selector.rs",
        "current.balances = rebuilt;"
    ));
    assert!(!is_field_replacement_violation(
        "src/lib.rs",
        "if supply == balances_snapshot { ok(); }"
    ));
    assert!(!is_field_replacement_violation(
        "src/lib.rs",
        "let cache = state_cache_from(&chain);"
    ));
}
