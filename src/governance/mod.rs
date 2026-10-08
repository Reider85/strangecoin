//! Governance-фасад P02: re-export SCIP-реализации из core (S1-P05).
//! Реальный код: `crates/strangecoin-core/src/governance/scip.rs`
//! (SCIP-документы, `ConsensusRules`, activation height; процесс — `docs/SCIP/`).
//! Не заглушка: monolith-потребители получают типы через этот re-export
//! (BUG-S0-020; см. также BUG-S0-006).
pub use strangecoin_core::governance::*;
