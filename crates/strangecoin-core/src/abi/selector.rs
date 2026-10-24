//! Method selectors (S1.5-P12, ADR-0013): first 4 bytes of
//! `blake3("name(type1,type2,...)")` — Ethereum layout, blake3 instead of
//! keccak. Golden vectors in `tests/abi_roundtrip.rs` pin stability.

use super::{AbiParam, AbiType};

/// Canonical signature string whose blake3 prefix is the selector.
pub fn method_signature(name: &str, params: &[AbiParam]) -> String {
    let types: Vec<String> = params
        .iter()
        .map(|p| p.param_type.canonical_name())
        .collect();
    format!("{name}({})", types.join(","))
}

/// First 4 bytes of `blake3(method_signature(name, params))`.
pub fn method_selector(name: &str, params: &[AbiParam]) -> [u8; 4] {
    let hash = blake3::hash(method_signature(name, params).as_bytes());
    let mut out = [0u8; 4];
    out.copy_from_slice(&hash.as_bytes()[..4]);
    out
}

/// Selector for a bare type list (no enclosing function name) — used by
/// event/error topic derivation in tooling.
pub fn types_selector(types: &[&AbiType]) -> [u8; 4] {
    let joined: Vec<String> = types.iter().map(|t| t.canonical_name()).collect();
    let hash = blake3::hash(joined.join(",").as_bytes());
    let mut out = [0u8; 4];
    out.copy_from_slice(&hash.as_bytes()[..4]);
    out
}
