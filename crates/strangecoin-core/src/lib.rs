pub mod abi;
pub mod address;
pub mod batch;
pub mod chain_selector;
pub mod consensus;
pub mod economics;
pub mod error;
/// EscrowVault calldata codec (AI-P03, ADR-0030) — shared by the ACP
/// handler, the WAT contract example and tests (see the module docs).
pub mod escrow;
pub mod governance;
pub mod serialize;
pub mod state;
pub mod types;
pub mod vm;

pub use chain_selector::{ChainInfo, ChainSelector};
pub use error::CoreError;
pub use serialize::{compute_receipts_root, compute_tx_root, merkle_root, serialize_receipt};
pub use state::witness::{
    build_witness, deserialize_state_witness, serialize_state_witness, verify_block_stateless,
    AccountProof, StateWitness, WITNESS_FORMAT_VERSION,
};
pub use state::{
    apply_block, apply_block_with_vm, compute_state_root, contract_storage_root, prove_receipt,
    receipts_root, receipts_subroot, root_after, root_after_with_vm, unapply_block,
    verify_receipt_proof, ContractStorage, ContractStorageMap, ContractStorageOverlay,
    SharedContractStorage, State,
};
pub use types::{
    Account, AccountState, Block, BlockHeader, CallTx, ChainSnapshot, ContractAccount, CreateTx,
    EoaAccount, Transaction, TransferTx,
};
pub use vm::{
    BlockContext, ExecutionContext, ExecutionResult, GasCounter, Log, Opcode, Receipt, TxStatus,
    ValueTransfer, VmError, VmExecutor,
};
