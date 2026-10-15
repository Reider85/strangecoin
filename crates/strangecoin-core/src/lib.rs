pub mod address;
pub mod chain_selector;
pub mod consensus;
pub mod economics;
pub mod error;
pub mod governance;
pub mod serialize;
pub mod state;
pub mod types;
pub mod vm;

pub use chain_selector::{ChainInfo, ChainSelector};
pub use error::CoreError;
pub use serialize::{compute_tx_root, merkle_root};
pub use state::witness::{build_witness, verify_block_stateless, AccountProof, StateWitness};
pub use state::{apply_block, compute_state_root, root_after, unapply_block, State};
pub use types::{
    Account, AccountState, Block, BlockHeader, CallTx, ChainSnapshot, ContractAccount, CreateTx,
    EoaAccount, Transaction, TransferTx,
};
pub use vm::{
    BlockContext, ExecutionResult, ExecutionContext, GasCounter, Log, Opcode, Receipt, TxStatus,
    VmError, VmExecutor,
};
