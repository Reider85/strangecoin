pub mod types;
pub mod serialize;
pub mod economics;
pub mod consensus;
pub mod address;
pub mod error;
pub mod state;
pub mod governance;

pub use types::{AccountState, Block, Transaction};
pub use error::CoreError;
pub use state::{apply_block, unapply_block, State, root_after, compute_state_root};
pub use serialize::{merkle_root, compute_tx_root};
pub use state::witness::{StateWitness, AccountProof, build_witness, verify_block_stateless};
