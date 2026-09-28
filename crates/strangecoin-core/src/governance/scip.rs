use std::collections::BTreeMap;

pub type Height = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScipStatus {
    Draft,
    Review,
    Active,
    Finalized,
}

#[derive(Debug, Clone)]
pub struct ScipDocument {
    pub scip: ScipId,
    pub title: String,
    pub status: ScipStatus,
    pub consensus_version: u32,
    pub activation_height: Option<Height>,
    pub author: String,
    pub discussions: String,
    pub created: String,
}

#[derive(Debug, Clone)]
pub struct ConsensusRules {
    pub consensus_version: u32,
    pub activations: BTreeMap<Height, u32>,
}

pub fn current_consensus_rules(height: Height, rules: &ConsensusRules) -> u32 {
    let mut active_version = rules.consensus_version;
    for (&activation_height, &new_version) in &rules.activations {
        if height >= activation_height {
            active_version = new_version;
        }
    }
    active_version
}

pub fn current_consensus_version(height: Height, activations: &BTreeMap<Height, u32>) -> u32 {
    let mut version: u32 = 1;
    for (&activation_height, &new_version) in activations {
        if height >= activation_height {
            version = new_version;
        }
    }
    version
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_activations_returns_initial_version() {
        let rules = ConsensusRules {
            consensus_version: 1,
            activations: BTreeMap::new(),
        };
        assert_eq!(current_consensus_rules(0, &rules), 1);
        assert_eq!(current_consensus_rules(100, &rules), 1);
        assert_eq!(current_consensus_rules(u64::MAX, &rules), 1);
    }

    #[test]
    fn activation_at_height_100() {
        let mut rules = ConsensusRules {
            consensus_version: 1,
            activations: BTreeMap::new(),
        };
        rules.activations.insert(100, 2);
        assert_eq!(current_consensus_rules(0, &rules), 1);
        assert_eq!(current_consensus_rules(99, &rules), 1);
        assert_eq!(current_consensus_rules(100, &rules), 2);
        assert_eq!(current_consensus_rules(1000, &rules), 2);
    }

    #[test]
    fn multiple_activations() {
        let mut rules = ConsensusRules {
            consensus_version: 1,
            activations: BTreeMap::new(),
        };
        rules.activations.insert(100, 2);
        rules.activations.insert(200, 3);
        assert_eq!(current_consensus_rules(0, &rules), 1);
        assert_eq!(current_consensus_rules(99, &rules), 1);
        assert_eq!(current_consensus_rules(100, &rules), 2);
        assert_eq!(current_consensus_rules(199, &rules), 2);
        assert_eq!(current_consensus_rules(200, &rules), 3);
        assert_eq!(current_consensus_rules(500, &rules), 3);
    }

    #[test]
    fn activation_boundary_exact_height() {
        let mut rules = ConsensusRules {
            consensus_version: 1,
            activations: BTreeMap::new(),
        };
        rules.activations.insert(50, 2);
        assert_eq!(current_consensus_rules(49, &rules), 1);
        assert_eq!(current_consensus_rules(50, &rules), 2);
    }

    #[test]
    fn current_consensus_version_standalone() {
        let mut activations = BTreeMap::new();
        activations.insert(100, 2);
        activations.insert(200, 3);
        assert_eq!(current_consensus_version(0, &activations), 1);
        assert_eq!(current_consensus_version(100, &activations), 2);
        assert_eq!(current_consensus_version(200, &activations), 3);
    }
}
