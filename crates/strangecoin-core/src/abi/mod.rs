//! JSON ABI (ADR-0013, S1.5-P12): document types, canonical argument
//! encode/decode, and blake3 method selectors.
//!
//! Two layers live side by side:
//!
//! - **Document layer** (this module + [`encode`] / [`decode`] / [`selector`]):
//!   `AbiDocument` describes a contract interface as a JSON-serializable
//!   value; calldata bytes are produced/consumed by the deterministic
//!   head/tail codec. `serde_json` parsing itself stays in tooling — core
//!   only owns the types and the byte rules (ADR-0013: "canonical binary
//!   encoding of calldata stays in core").
//! - **Executor-flags layer** ([`AbiEntry`] / [`ContractAbi`]): the minimal
//!   per-selector metadata the VM consults for the `non_reentrant` modifier
//!   (S1.5-P13, threat V-18). Unchanged by this module.
//!
//! Wire format (fixed by golden vectors in `tests/abi_roundtrip.rs`):
//!
//! - Static scalars (`uintN`/`intN`/`bool`/`address`) are 32-byte
//!   big-endian words, Ethereum-style.
//! - Dynamic values (`bytes`, `string`, `T[]`, and any tuple/fixed array
//!   containing them) are head/tail-split: the head carries a **4-byte
//!   big-endian offset** from the start of the enclosing tuple, the tail
//!   carries a **4-byte big-endian length** (or element count) followed by
//!   the body. Offsets must be canonical (equal to the sequential tail
//!   cursor) — shuffled layouts are rejected on decode.
//! - `format_version: "json-v1"` is mandatory on [`AbiDocument`];
//!   `"wit-v2"` is reserved for the Stage 4+ WIT migration and fails with
//!   [`AbiError::NotImplemented`] until a SCIP activates it.

use serde::{Deserialize, Serialize};

pub mod decode;
pub mod encode;
pub mod selector;

pub use decode::decode_args;
pub use encode::encode_args;
pub use selector::{method_selector, method_signature};

/// Current ABI document format version this node implements.
pub const ABI_FORMAT_VERSION: &str = "json-v1";

/// Reserved future WIT-based format (Stage 4+, ADR-0013). Not implemented.
pub const ABI_FORMAT_VERSION_WIT_V2: &str = "wit-v2";

/// One callable entry of a contract's ABI (executor-flags layer, S1.5-P13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiEntry {
    /// Function name (informational until P12 dispatch lands).
    pub name: String,
    /// First 4 bytes of the canonical call data for this entry.
    pub selector: [u8; 4],
    /// `nonReentrant` modifier: entering this function while the contract is
    /// already on the active call stack must fail with
    /// [`VmError::ReentrancyDetected`](crate::vm::VmError::ReentrancyDetected).
    pub non_reentrant: bool,
}

impl AbiEntry {
    pub fn new(name: impl Into<String>, selector: [u8; 4], non_reentrant: bool) -> Self {
        Self {
            name: name.into(),
            selector,
            non_reentrant,
        }
    }

    /// Selector for a parameterless function — convenience for the
    /// S1.5-P13 reentrancy-guard layer, where callers build entries by name
    /// (S1.5-FBP02 КГ).
    pub fn selector_from_name(name: &str) -> [u8; 4] {
        method_selector(name, &[])
    }
}

/// ABI of one contract: required format version + callable entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractAbi {
    pub format_version: String,
    pub entries: Vec<AbiEntry>,
}

impl ContractAbi {
    pub fn new(entries: Vec<AbiEntry>) -> Self {
        Self {
            format_version: ABI_FORMAT_VERSION.to_string(),
            entries,
        }
    }

    /// Look up the entry a 4-byte selector resolves to.
    pub fn entry_by_selector(&self, selector: [u8; 4]) -> Option<&AbiEntry> {
        self.entries.iter().find(|e| e.selector == selector)
    }
}

/// Codec / document error for the JSON ABI layer (S1.5-P12).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AbiError {
    #[error("argument count mismatch: expected {expected}, got {got}")]
    ArityMismatch { expected: usize, got: usize },
    #[error("type mismatch: expected {expected}, got {got}")]
    TypeMismatch { expected: String, got: String },
    #[error("invalid integer width {0} (must be 8..=256, multiple of 8)")]
    InvalidBits(u16),
    #[error("value out of range for declared type: {reason}")]
    InvalidValue { reason: String },
    #[error("unexpected end of encoded data at byte {0}")]
    UnexpectedEnd(usize),
    #[error("trailing data after decode: {remaining} bytes at offset {at}")]
    TrailingData { at: usize, remaining: usize },
    #[error("offset {offset} out of bounds (tuple body is {bound} bytes)")]
    InvalidOffset { offset: u32, bound: usize },
    #[error("non-canonical offset: expected {expected}, got {got}")]
    NonCanonicalOffset { expected: u32, got: u32 },
    #[error("string is not valid UTF-8")]
    InvalidUtf8,
    #[error("unknown ABI type name: {found}")]
    InvalidType { found: String },
    #[error("ABI format version {found:?} is reserved but not implemented")]
    NotImplemented { found: String },
    #[error("unknown ABI format version: {found:?}")]
    UnknownFormatVersion { found: String },
}

/// One callable/observable entry parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: AbiType,
}

impl AbiParam {
    pub fn new(name: impl Into<String>, param_type: AbiType) -> Self {
        Self {
            name: name.into(),
            param_type,
        }
    }
}

/// Event input parameter: like [`AbiParam`] plus the `indexed` flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiEventParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: AbiType,
    #[serde(default)]
    pub indexed: bool,
}

/// Ethereum-style state mutability (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateMutability {
    Pure,
    View,
    Nonpayable,
    Payable,
}

/// A function entry of an ABI document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiFunction {
    pub name: String,
    #[serde(default)]
    pub inputs: Vec<AbiParam>,
    #[serde(default)]
    pub outputs: Vec<AbiParam>,
    pub state_mutability: StateMutability,
}

impl AbiFunction {
    /// First 4 bytes of `blake3("name(type1,type2,...)")` (S1.5-P12).
    pub fn selector(&self) -> [u8; 4] {
        method_selector(&self.name, &self.inputs)
    }
}

/// An event entry of an ABI document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiEvent {
    pub name: String,
    #[serde(default)]
    pub inputs: Vec<AbiEventParam>,
}

/// A custom-error entry of an ABI document.
///
/// Named `AbiErrorDecl` (not `AbiError`) because the latter is the codec
/// error type of this module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiErrorDecl {
    pub name: String,
    #[serde(default)]
    pub inputs: Vec<AbiParam>,
}

/// A full contract ABI document (`format_version: "json-v1"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiDocument {
    /// Mandatory discriminator; must equal [`ABI_FORMAT_VERSION`] today.
    pub format_version: String,
    pub name: String,
    #[serde(default)]
    pub functions: Vec<AbiFunction>,
    #[serde(default)]
    pub events: Vec<AbiEvent>,
    #[serde(default)]
    pub errors: Vec<AbiErrorDecl>,
}

impl AbiDocument {
    pub fn new(name: impl Into<String>, functions: Vec<AbiFunction>) -> Self {
        Self {
            format_version: ABI_FORMAT_VERSION.to_string(),
            name: name.into(),
            functions,
            events: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Enforce the `format_version` discriminator. `"json-v1"` passes,
    /// `"wit-v2"` → [`AbiError::NotImplemented`] (Stage 4+ migration),
    /// anything else → [`AbiError::UnknownFormatVersion`].
    pub fn check_format_version(&self) -> Result<(), AbiError> {
        match self.format_version.as_str() {
            v if v == ABI_FORMAT_VERSION => Ok(()),
            v if v == ABI_FORMAT_VERSION_WIT_V2 => Err(AbiError::NotImplemented {
                found: v.to_string(),
            }),
            other => Err(AbiError::UnknownFormatVersion {
                found: other.to_string(),
            }),
        }
    }

    pub fn function_by_name(&self, name: &str) -> Option<&AbiFunction> {
        self.functions.iter().find(|f| f.name == name)
    }

    pub fn function_by_selector(&self, selector: [u8; 4]) -> Option<&AbiFunction> {
        self.functions.iter().find(|f| f.selector() == selector)
    }
}

/// ABI value used by the argument codec.
///
/// The variant carries the runtime value; the *declared* type comes from the
/// enclosing [`AbiParam`] and is validated on encode/decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiValue {
    /// Unsigned integer, 32-byte big-endian (canonical, range-checked
    /// against the declared `uintN` width on encode).
    Uint([u8; 32]),
    /// Signed integer, 32-byte big-endian two's complement (canonical,
    /// sign-extended to 256 bits on encode).
    Int([u8; 32]),
    Bool(bool),
    /// 32-byte account address.
    Address([u8; 32]),
    Bytes(Vec<u8>),
    String(String),
    /// Dynamic-length array (`T[]`).
    Array(Vec<AbiValue>),
    /// Fixed-length array (`T[len]`).
    FixedArray(Vec<AbiValue>),
    Tuple(Vec<AbiValue>),
}

impl AbiValue {
    /// Convenience: `uint256` from a `u64` (high bytes zeroed).
    pub fn uint_u64(v: u64) -> Self {
        let mut b = [0u8; 32];
        b[24..].copy_from_slice(&v.to_be_bytes());
        AbiValue::Uint(b)
    }

    /// Convenience: `int256` from an `i64` (sign-extended).
    pub fn int_i64(v: i64) -> Self {
        let mut b = [0u8; 32];
        if v < 0 {
            b = [0xff; 32];
        }
        b[24..].copy_from_slice(&v.to_be_bytes());
        AbiValue::Int(b)
    }
}

/// A contract ABI type (canonical string form, serde as string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiType {
    /// `uintN`, N ∈ {8, 16, …, 256}.
    Uint(u16),
    /// `intN`, N ∈ {8, 16, …, 256}.
    Int(u16),
    Bool,
    /// 32-byte account address.
    Address,
    /// Dynamic byte string.
    Bytes,
    /// UTF-8 string.
    String,
    /// Dynamic-length array `T[]`.
    Array(Box<AbiType>),
    /// Fixed-length array `T[len]`.
    FixedArray(Box<AbiType>, usize),
    /// Structural tuple `(T1,T2,...)`.
    Tuple(Vec<AbiType>),
}

impl AbiType {
    /// Validate an integer width: 8..=256, multiple of 8.
    pub fn validate_bits(bits: u16) -> Result<(), AbiError> {
        if (8..=256).contains(&bits) && bits.is_multiple_of(8) {
            Ok(())
        } else {
            Err(AbiError::InvalidBits(bits))
        }
    }

    /// True iff values of this type use the tail section of their tuple.
    pub fn is_dynamic(&self) -> bool {
        match self {
            AbiType::Uint(_) | AbiType::Int(_) | AbiType::Bool | AbiType::Address => false,
            AbiType::Bytes | AbiType::String | AbiType::Array(_) => true,
            AbiType::FixedArray(elem, _) => elem.is_dynamic(),
            AbiType::Tuple(elems) => elems.iter().any(Self::is_dynamic),
        }
    }

    /// Byte size of the head contribution of a static type; `None` for
    /// dynamic types.
    pub fn static_size(&self) -> Option<usize> {
        match self {
            AbiType::Uint(_) | AbiType::Int(_) => Some(32),
            AbiType::Bool => Some(32),
            AbiType::Address => Some(32),
            AbiType::FixedArray(elem, len) => Some(elem.static_size()? * len),
            AbiType::Tuple(elems) => {
                let mut total = 0usize;
                for e in elems {
                    total += e.static_size()?;
                }
                Some(total)
            }
            AbiType::Bytes | AbiType::String | AbiType::Array(_) => None,
        }
    }

    /// Canonical signature string (`"uint256"`, `"bytes"`, `"u64[]"`,
    /// `"(bool,address)"`, …) — the selector preimage component.
    pub fn canonical_name(&self) -> String {
        match self {
            AbiType::Uint(n) => format!("uint{n}"),
            AbiType::Int(n) => format!("int{n}"),
            AbiType::Bool => "bool".to_string(),
            AbiType::Address => "address".to_string(),
            AbiType::Bytes => "bytes".to_string(),
            AbiType::String => "string".to_string(),
            AbiType::Array(elem) => format!("{}[]", elem.canonical_name()),
            AbiType::FixedArray(elem, len) => format!("{}[{len}]", elem.canonical_name()),
            AbiType::Tuple(elems) => {
                let inner: Vec<String> = elems.iter().map(Self::canonical_name).collect();
                format!("({})", inner.join(","))
            }
        }
    }

    /// Parse a canonical type string (inverse of [`AbiType::canonical_name`]).
    pub fn parse(s: &str) -> Result<Self, AbiError> {
        let invalid = || AbiError::InvalidType {
            found: s.to_string(),
        };

        if let Some(inner) = s.strip_suffix("[]") {
            return Ok(AbiType::Array(Box::new(AbiType::parse(inner)?)));
        }
        if s.ends_with(']') {
            if let Some(open) = s.rfind('[') {
                let elem = &s[..open];
                let len_s = &s[open + 1..s.len() - 1];
                if !len_s.is_empty() && len_s.bytes().all(|b| b.is_ascii_digit()) {
                    let len: usize = len_s.parse().map_err(|_| invalid())?;
                    // FixedArray length must be > 0 (S1.5-FBP01 spec, task 2).
                    if len == 0 {
                        return Err(invalid());
                    }
                    return Ok(AbiType::FixedArray(
                        Box::new(AbiType::parse(elem)?),
                        len,
                    ));
                }
            }
            return Err(invalid());
        }
        if s.starts_with('(') && s.ends_with(')') {
            let inner = &s[1..s.len() - 1];
            if inner.is_empty() {
                return Ok(AbiType::Tuple(Vec::new()));
            }
            let mut parts = Vec::new();
            let mut depth = 0usize;
            let mut start = 0usize;
            for (i, ch) in inner.char_indices() {
                match ch {
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth = depth.saturating_sub(1),
                    ',' if depth == 0 => {
                        parts.push(&inner[start..i]);
                        start = i + 1;
                    }
                    _ => {}
                }
            }
            parts.push(&inner[start..]);
            let mut elems = Vec::with_capacity(parts.len());
            for p in parts {
                elems.push(AbiType::parse(p)?);
            }
            return Ok(AbiType::Tuple(elems));
        }
        match s {
            "bool" => return Ok(AbiType::Bool),
            "address" => return Ok(AbiType::Address),
            "bytes" => return Ok(AbiType::Bytes),
            "string" => return Ok(AbiType::String),
            _ => {}
        }
        if let Some(n) = s.strip_prefix("uint") {
            let bits: u16 = n.parse().map_err(|_| invalid())?;
            AbiType::validate_bits(bits).map_err(|_| invalid())?;
            return Ok(AbiType::Uint(bits));
        }
        if let Some(n) = s.strip_prefix("int") {
            let bits: u16 = n.parse().map_err(|_| invalid())?;
            AbiType::validate_bits(bits).map_err(|_| invalid())?;
            return Ok(AbiType::Int(bits));
        }
        Err(invalid())
    }
}

impl Serialize for AbiType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.canonical_name())
    }
}

impl<'de> Deserialize<'de> for AbiType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        AbiType::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_name_parse_roundtrip_for_all_variants() {
        let cases = [
            "uint8",
            "uint64",
            "uint256",
            "int8",
            "int128",
            "int256",
            "bool",
            "address",
            "bytes",
            "string",
            "bytes[]",
            "uint64[3]",
            "(bool,address)",
            "(uint8,(bool,string)[])",
            "()",
        ];
        for c in cases {
            let t = AbiType::parse(c).expect(c);
            assert_eq!(t.canonical_name(), c, "canonical form of {c}");
        }
    }

    #[test]
    fn parse_rejects_invalid_types() {
        assert!(AbiType::parse("uint7").is_err());
        assert!(AbiType::parse("uint264").is_err());
        assert!(AbiType::parse("float").is_err());
        assert!(AbiType::parse("uint64[").is_err());
        assert!(AbiType::parse("(bool").is_err());
        assert!(AbiType::parse("bool[0x3]").is_err());
        // FixedArray length must be > 0 (S1.5-FBP01).
        assert!(AbiType::parse("uint64[0]").is_err());
        assert!(AbiType::parse("bool[0]").is_err());
    }

    #[test]
    fn dynamic_classification() {
        assert!(!AbiType::Uint(256).is_dynamic());
        assert!(!AbiType::FixedArray(Box::new(AbiType::Bool), 4).is_dynamic());
        assert!(AbiType::Bytes.is_dynamic());
        assert!(AbiType::Array(Box::new(AbiType::Uint(8))).is_dynamic());
        assert!(AbiType::FixedArray(Box::new(AbiType::Bytes), 2).is_dynamic());
        assert!(AbiType::Tuple(vec![AbiType::Bool, AbiType::String]).is_dynamic());
        assert!(!AbiType::Tuple(vec![AbiType::Bool, AbiType::Address]).is_dynamic());
    }

    #[test]
    fn static_sizes() {
        assert_eq!(AbiType::Uint(8).static_size(), Some(32));
        assert_eq!(AbiType::Bool.static_size(), Some(32));
        assert_eq!(
            AbiType::FixedArray(Box::new(AbiType::Uint(8)), 3).static_size(),
            Some(96)
        );
        assert_eq!(
            AbiType::Tuple(vec![AbiType::Bool, AbiType::Address]).static_size(),
            Some(64)
        );
        assert_eq!(AbiType::String.static_size(), None);
        assert_eq!(AbiType::FixedArray(Box::new(AbiType::Bytes), 2).static_size(), None);
    }

    #[test]
    fn entry_selector_from_name_matches_golden_vector() {
        // Golden vector, same pin as tests/abi_roundtrip.rs
        // (method_selector("totalSupply", &[])).
        assert_eq!(AbiEntry::selector_from_name("totalSupply"), [0x1e, 0x92, 0x6a, 0x5b]);
        assert_eq!(
            AbiEntry::selector_from_name("totalSupply"),
            method_selector("totalSupply", &[])
        );
    }

    #[test]
    fn format_version_gating() {
        let ok = AbiDocument::new("C", Vec::new());
        assert_eq!(ok.check_format_version(), Ok(()));

        let mut wit = AbiDocument::new("C", Vec::new());
        wit.format_version = ABI_FORMAT_VERSION_WIT_V2.to_string();
        assert_eq!(
            wit.check_format_version(),
            Err(AbiError::NotImplemented {
                found: "wit-v2".to_string()
            })
        );

        let mut bad = AbiDocument::new("C", Vec::new());
        bad.format_version = "json-v0".to_string();
        assert_eq!(
            bad.check_format_version(),
            Err(AbiError::UnknownFormatVersion {
                found: "json-v0".to_string()
            })
        );
    }

    #[test]
    fn document_function_lookup() {
        let f = AbiFunction {
            name: "transfer".to_string(),
            inputs: vec![
                AbiParam::new("to", AbiType::Address),
                AbiParam::new("amount", AbiType::Uint(256)),
            ],
            outputs: vec![],
            state_mutability: StateMutability::Nonpayable,
        };
        let sel = f.selector();
        let doc = AbiDocument::new("Token", vec![f]);
        assert!(doc.function_by_name("transfer").is_some());
        assert!(doc.function_by_name("approve").is_none());
        assert_eq!(doc.function_by_selector(sel).map(|f| f.name.as_str()), Some("transfer"));
    }
}
