use strangecoin_core::vm::{GasCounter, VmError};
use wasmi::Caller;

/// Per-host-call gas weights (ADR-0014 sketch; SCIP-gated after mainnet freeze).
///
/// The enum covers every host function of S1.5-P10 plus engine-visible
/// memory growth, so metering is defined before the functions land. Variable
/// inputs carry their size; `gas()` applies the ADR-0014/0015 formulas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostCallWeight {
    /// `secp256k1_verify` — 3000 (mirrors precompile `ecrecover`, ADR-0015).
    Secp256k1Verify,
    /// `bls_verify` — 12000 (BLS12-381 pairing estimate, reserved until Stage 7).
    BlsVerify,
    /// `blake3` — 50 + 6 per 64-byte chunk.
    Blake3 { bytes: u64 },
    /// `keccak` — 30 + 6 per 32-byte word.
    Keccak { bytes: u64 },
    /// `sha256` — 50 + 12 per 32-byte word.
    Sha256 { bytes: u64 },
    /// `storage_read` — 200 (overlay lookup + future SMT proof verification).
    StorageRead,
    /// `storage_write` — 5000 (SMT update must hurt).
    StorageWrite,
    /// `call` — 700 base (sketch), plus the child frame's own gas budget.
    Call,
    /// `delegatecall` — 700 base (sketch).
    DelegateCall,
    /// `staticcall` — 700 base (sketch).
    StaticCall,
    /// `block_context` — 3.
    BlockContext,
    /// `gas_left` — 3.
    GasLeft,
    /// `emit_log` — 375 + 375 per topic.
    EmitLog { topics: u64 },
    /// One 64KiB `memory.grow` page — 3 (ADR-0014 memory metering).
    MemoryGrow,
}

impl HostCallWeight {
    /// Weight in gas units per ADR-0014/0015.
    pub fn gas(self) -> u64 {
        match self {
            HostCallWeight::Secp256k1Verify => 3000,
            HostCallWeight::BlsVerify => 12000,
            HostCallWeight::Blake3 { bytes } => 50 + 6 * bytes.div_ceil(64),
            HostCallWeight::Keccak { bytes } => 30 + 6 * bytes.div_ceil(32),
            HostCallWeight::Sha256 { bytes } => 50 + 12 * bytes.div_ceil(32),
            HostCallWeight::StorageRead => 200,
            HostCallWeight::StorageWrite => 5000,
            HostCallWeight::Call => 700,
            HostCallWeight::DelegateCall => 700,
            HostCallWeight::StaticCall => 700,
            HostCallWeight::BlockContext => 3,
            HostCallWeight::GasLeft => 3,
            HostCallWeight::EmitLog { topics } => 375 + 375 * topics,
            HostCallWeight::MemoryGrow => 3,
        }
    }
}

/// Standalone gas budget implementing the core [`GasCounter`] trait.
///
/// The wasmi store's own fuel counter meters instructions inside the engine;
/// this struct mirrors the same budget semantics as pure logic so counter
/// behaviour (spend / underflow / host-call weights) is testable without a
/// runtime, and so host functions (S1.5-P10) share one weight table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuelGasCounter {
    limit: u64,
    spent: u64,
}

impl FuelGasCounter {
    pub fn new(limit: u64) -> Self {
        Self { limit, spent: 0 }
    }

    /// Charge a host call at `weight.gas()`, failing with
    /// [`VmError::OutOfGas`] when the budget cannot cover it.
    pub fn spend_host_call(&mut self, weight: HostCallWeight) -> Result<(), VmError> {
        self.spend(weight.gas())
    }
}

impl GasCounter for FuelGasCounter {
    fn spend(&mut self, gas: u64) -> Result<(), VmError> {
        let Some(next) = self.spent.checked_add(gas) else {
            return Err(VmError::OutOfGas);
        };
        if next > self.limit {
            return Err(VmError::OutOfGas);
        }
        self.spent = next;
        Ok(())
    }

    fn gas_left(&self) -> u64 {
        self.limit - self.spent
    }

    fn gas_used(&self) -> u64 {
        self.spent
    }
}

/// Deduct `weight` from the wasmi store's fuel before performing host work.
///
/// This is the in-engine counterpart of [`FuelGasCounter::spend_host_call`]:
/// a single fuel counter meters instructions (engine) and host calls (this
/// function). Insufficient fuel traps with `TrapCode::OutOfFuel`, which the
/// executor maps to [`VmError::OutOfGas`].
pub fn charge_host_call<T>(caller: &mut Caller<'_, T>, weight: u64) -> Result<(), wasmi::Error> {
    let fuel = caller.get_fuel()?;
    if fuel < weight {
        return Err(wasmi::Error::from(wasmi::TrapCode::OutOfFuel));
    }
    caller.set_fuel(fuel - weight)?;
    Ok(())
}
