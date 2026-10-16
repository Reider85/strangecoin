//! Execution limits (S1.5-P13, ARCHITECT3 §3.9, threats V-18..V-21).
//!
//! Constants live in core (runtime-free) so consensus-level checks
//! (`max_code_size` in `verify_transaction`) can enforce them without the
//! `vm-wasm` feature, mirroring `BLOCK_GAS_LIMIT`.

/// Maximum `init_code` length of a `CreateTx` (24 KB, threat V-19/V-21:
/// a single deploy must not carry unbounded code into the chain).
pub const MAX_CODE_SIZE: usize = 24 * 1024;

/// Maximum total value bytes staged by one contract's storage overlay
/// (1 MB, threat V-20: storage exhaustion). Enforced in the
/// `storage_write` host function.
pub const MAX_STORAGE_PER_CONTRACT: usize = 1024 * 1024;

/// Maximum nesting of `call`/`delegatecall`/`staticcall` frames (threat
/// V-18/V-21: unbounded recursion must not exhaust the host stack).
pub const MAX_CALL_DEPTH: u32 = 1024;

/// Maximum linear-memory pages per contract frame (16 × 64 KiB = 1 MiB).
/// Enforced through the wasmi `StoreLimits` resource limiter.
pub const MAX_MEMORY_PAGES: u32 = 16;

/// [`MAX_MEMORY_PAGES`](MAX_MEMORY_PAGES) expressed in bytes.
pub const MAX_MEMORY_BYTES: usize = MAX_MEMORY_PAGES as usize * 65_536;

/// Maximum height of the wasmi value stack in bytes. Set explicitly on the
/// engine config so runaway operand-stack growth traps deterministically.
pub const MAX_STACK_HEIGHT: usize = 1_000_000;
