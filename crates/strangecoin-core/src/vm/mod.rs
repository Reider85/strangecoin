pub mod context;
pub mod error;
pub mod gas;
pub mod limits;
pub mod result;
pub mod traits;

pub use context::{BlockContext, ExecutionContext};
pub use error::VmError;
pub use gas::{GasCounter, BLOCK_GAS_LIMIT};
pub use limits::{
    MAX_CALL_DEPTH, MAX_CODE_SIZE, MAX_MEMORY_BYTES, MAX_MEMORY_PAGES, MAX_STACK_HEIGHT,
    MAX_STORAGE_PER_CONTRACT,
};
pub use result::{ExecutionResult, Log, Receipt, TxStatus};
pub use traits::{Opcode, VmExecutor};
