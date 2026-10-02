//! # Consensus manager (ARCHITECT3 §3.4, component 5)
//!
//! The single component that decides *which* consensus rules apply at a given
//! height. Everything else — `block_executor` above all — receives the rules
//! from here and never resolves them on its own.
//!
//! The mechanism itself lives in `strangecoin_core::governance` (S1-P05):
//! `ConsensusRules` + activation heights. This manager wraps it, adds the
//! PoW/PoS phase dimension and keeps a single default instance for the
//! current chain.

use strangecoin_core::consensus::CURRENT_CONSENSUS_VERSION;
use strangecoin_core::governance::{current_consensus_rules, ConsensusRules, Height};

/// Which consensus family is active at a height.
///
/// `Pos` is a placeholder until Stage 7: the variant exists so the type
/// system can carry the distinction from day one, but no validation path
/// accepts it yet (`block_executor` rejects it explicitly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsensusPhase {
    Pow,
    Pos,
}

/// Resolves active consensus rules per height and the phase active there.
#[derive(Debug, Clone)]
pub struct ConsensusManager {
    rules: ConsensusRules,
    pos_activation_height: Option<Height>,
}

impl Default for ConsensusManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusManager {
    /// Chain rules as of Stage 1: version from the core constant, no
    /// pending activations, PoW-only (no PoS activation height).
    pub fn new() -> Self {
        Self {
            rules: ConsensusRules {
                consensus_version: CURRENT_CONSENSUS_VERSION,
                activations: std::collections::BTreeMap::new(),
            },
            pos_activation_height: None,
        }
    }

    pub fn with_rules(rules: ConsensusRules) -> Self {
        Self {
            rules,
            pos_activation_height: None,
        }
    }

    /// Set the height at which the phase switches to PoS (Stage 7 will be
    /// the first caller with `Some(_)`).
    pub fn with_pos_activation(mut self, height: Height) -> Self {
        self.pos_activation_height = Some(height);
        self
    }

    /// Consensus version a block at `height` must carry.
    pub fn expected_version(&self, height: Height) -> u32 {
        current_consensus_rules(height, &self.rules)
    }

    /// Phase in force at `height`.
    pub fn phase_at(&self, height: Height) -> ConsensusPhase {
        match self.pos_activation_height {
            Some(h) if height >= h => ConsensusPhase::Pos,
            _ => ConsensusPhase::Pow,
        }
    }

    pub fn rules(&self) -> &ConsensusRules {
        &self.rules
    }

    pub fn pos_activation_height(&self) -> Option<Height> {
        self.pos_activation_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_manager_expects_current_version_at_every_height() {
        let manager = ConsensusManager::new();
        assert_eq!(manager.expected_version(0), CURRENT_CONSENSUS_VERSION);
        assert_eq!(manager.expected_version(1), CURRENT_CONSENSUS_VERSION);
        assert_eq!(
            manager.expected_version(u64::MAX),
            CURRENT_CONSENSUS_VERSION
        );
    }

    #[test]
    fn activation_switches_expected_version_by_height() {
        let mut rules = ConsensusRules {
            consensus_version: CURRENT_CONSENSUS_VERSION,
            activations: std::collections::BTreeMap::new(),
        };
        rules.activations.insert(100, 2);
        let manager = ConsensusManager::with_rules(rules);
        assert_eq!(manager.expected_version(99), 1);
        assert_eq!(manager.expected_version(100), 2);
        assert_eq!(manager.expected_version(101), 2);
    }

    #[test]
    fn phase_is_pow_until_pos_activation_height() {
        let manager = ConsensusManager::new().with_pos_activation(1000);
        assert_eq!(manager.phase_at(0), ConsensusPhase::Pow);
        assert_eq!(manager.phase_at(999), ConsensusPhase::Pow);
        assert_eq!(manager.phase_at(1000), ConsensusPhase::Pos);
        assert_eq!(manager.phase_at(5000), ConsensusPhase::Pos);
    }

    #[test]
    fn default_phase_is_pow_everywhere() {
        let manager = ConsensusManager::new();
        assert_eq!(manager.phase_at(0), ConsensusPhase::Pow);
        assert_eq!(manager.phase_at(u64::MAX), ConsensusPhase::Pow);
        assert!(manager.pos_activation_height().is_none());
    }
}
