//! Shadow network tooling (S2-P20, Stage 6 prep): replay mainnet block
//! history against a staging chain and detect consensus divergence —
//! the upgrade-testing harness ROADMAP3 «Этап 2» defers to the Stage 6
//! security track (external audit / formal verification prep).
//!
//! [`replay`] loads an exported mainnet block file and submits each
//! block through [`BlockchainView::add_block`]; [`compare`] flags every
//! block whose accept/reject status differs between the mainnet history
//! and the staging replay. See `analytics/prompt-stage2.md` S2-P20.

pub mod compare;
pub mod replay;

pub use compare::{compare, BlockResult, BlockStatus, DivergenceReport};
pub use replay::{load_blocks_file, replay_blocks, ReplayConfig, ReplayReport};
