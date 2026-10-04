use crate::consensus::{cumulative_work, u256_gt, U256};
use crate::types::Block;

#[derive(Clone, Debug)]
pub struct ChainInfo {
    pub tip_height: u64,
    pub tip_hash: String,
    pub total_work: U256,
    pub tip_timestamp: u64,
}

pub struct ChainSelector;

impl ChainSelector {
    pub fn select_best<'a>(chains: &[&'a ChainInfo]) -> Option<&'a ChainInfo> {
        chains.iter().copied().reduce(|best, candidate| {
            if Self::is_better(candidate, best) {
                candidate
            } else {
                best
            }
        })
    }

    pub fn is_better(a: &ChainInfo, b: &ChainInfo) -> bool {
        if u256_gt(a.total_work, b.total_work) {
            return true;
        }
        if u256_gt(b.total_work, a.total_work) {
            return false;
        }
        if a.tip_timestamp < b.tip_timestamp {
            return true;
        }
        if b.tip_timestamp < a.tip_timestamp {
            return false;
        }
        a.tip_hash < b.tip_hash
    }

    pub fn chain_info(chain: &[Block]) -> Option<ChainInfo> {
        let tip = chain.last()?;
        let tip_hash = tip.hash.clone();
        let tip_height = tip.index;
        let tip_timestamp = tip.timestamp;
        let total_work = cumulative_work(chain);
        Some(ChainInfo {
            tip_height,
            tip_hash,
            total_work,
            tip_timestamp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(index: u64, target_hex: &str, timestamp: u64, hash: &str) -> Block {
        Block {
            index,
            timestamp,
            transactions: vec![],
            previous_hash: String::new(),
            hash: hash.to_string(),
            nonce: 0,
            target: target_hex.to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        }
    }

    #[test]
    fn select_best_prefers_higher_work() {
        let c1 = ChainInfo {
            tip_height: 5,
            tip_hash: "aaa".into(),
            total_work: [0, 0, 0, 100],
            tip_timestamp: 1000,
        };
        let c2 = ChainInfo {
            tip_height: 5,
            tip_hash: "bbb".into(),
            total_work: [0, 0, 0, 200],
            tip_timestamp: 1000,
        };
        let best = ChainSelector::select_best(&[&c1, &c2]).unwrap();
        assert_eq!(best.total_work, [0, 0, 0, 200]);
    }

    #[test]
    fn select_best_prefers_lower_timestamp() {
        let c1 = ChainInfo {
            tip_height: 5,
            tip_hash: "aaa".into(),
            total_work: [0, 0, 0, 100],
            tip_timestamp: 2000,
        };
        let c2 = ChainInfo {
            tip_height: 5,
            tip_hash: "bbb".into(),
            total_work: [0, 0, 0, 100],
            tip_timestamp: 1000,
        };
        let best = ChainSelector::select_best(&[&c1, &c2]).unwrap();
        assert_eq!(best.tip_hash, "bbb");
    }

    #[test]
    fn select_best_prefers_lower_hash_on_tie() {
        let c1 = ChainInfo {
            tip_height: 5,
            tip_hash: "aaa".into(),
            total_work: [0, 0, 0, 100],
            tip_timestamp: 1000,
        };
        let c2 = ChainInfo {
            tip_height: 5,
            tip_hash: "bbb".into(),
            total_work: [0, 0, 0, 100],
            tip_timestamp: 1000,
        };
        let best = ChainSelector::select_best(&[&c1, &c2]).unwrap();
        assert_eq!(best.tip_hash, "aaa");
    }

    #[test]
    fn chain_info_from_blocks() {
        const TARGET_HEX: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let block1 = make_block(1, TARGET_HEX, 1000, "hash1");
        let block2 = make_block(2, TARGET_HEX, 1500, "hash2");
        let chain = vec![block1, block2];
        let info = ChainSelector::chain_info(&chain).unwrap();
        assert_eq!(info.tip_height, 2);
        assert_eq!(info.tip_hash, "hash2");
        assert_eq!(info.tip_timestamp, 1500);
    }
}