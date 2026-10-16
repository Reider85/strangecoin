//! Minimal ABI stub (S1.5-P13): just enough contract metadata for the
//! `non_reentrant` modifier pattern (threat V-18).
//!
//! The full JSON ABI — `AbiDocument` with encode/decode and selector-based
//! dispatch — ships with S1.5-P12; this stub only carries per-entry flags
//! the executor consults before a nested call. `ContractAbi` instances are
//! registered on the executor (`WasmiExecutor::insert_abi`) and looked up
//! by target address + 4-byte calldata selector.

/// ABI document format version this node understands (S1.5-P12 reserves
/// `"wit-v2"` for a future migration).
pub const ABI_FORMAT_VERSION: &str = "json-v1";

/// One callable entry of a contract's ABI.
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
