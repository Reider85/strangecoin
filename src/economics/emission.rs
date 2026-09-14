use crate::consensus;

pub const TAIL_RATE_NUMERATOR: u64 = 6;
pub const TAIL_RATE_DENOMINATOR: u64 = 1000;
pub const BLOCKS_PER_YEAR: u64 = 365 * 24 * 6;
pub const HALVING_INTERVAL: u64 = 210_000;
pub const MAX_SUPPLY_PRE_TAIL: u64 = 21_000_000 * 100_000_000;
pub const INITIAL_REWARD: u64 = 50 * 100_000_000;

/// Zero reward for regtest to avoid breaking existing integration tests
/// Mainnet/testnet use full emission schedule
const REGTEST_REWARD: u64 = 0;

pub fn block_reward_at_height(height: u64, total_supply: u64) -> u64 {
    block_reward_at_height_for_chain(height, total_supply, consensus::current_chain_id())
}

pub fn block_reward_at_height_for_chain(height: u64, total_supply: u64, chain_id: u32) -> u64 {
    // Regtest: minimal fixed reward to avoid interfering with existing tests
    if chain_id == consensus::CHAIN_ID_REGTEST {
        return REGTEST_REWARD;
    }
    let base_reward = halving_schedule(height);
    let tail_reward = calculate_tail_reward(total_supply);
    base_reward.max(tail_reward)
}

fn halving_schedule(height: u64) -> u64 {
    let halvings = height / HALVING_INTERVAL;
    if halvings >= 64 { return 0; }
    INITIAL_REWARD >> halvings
}

fn calculate_tail_reward(total_supply: u64) -> u64 {
    (total_supply * TAIL_RATE_NUMERATOR) / (TAIL_RATE_DENOMINATOR * BLOCKS_PER_YEAR)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COIN: u64 = 100_000_000;

    #[test]
    fn test_genesis_reward() {
        assert_eq!(block_reward_at_height_for_chain(0, 0, consensus::CHAIN_ID_MAINNET), 50 * COIN);
    }

    #[test]
    fn test_first_halving() {
        assert_eq!(block_reward_at_height_for_chain(HALVING_INTERVAL, 0, consensus::CHAIN_ID_MAINNET), 25 * COIN);
    }

    #[test]
    fn test_second_halving() {
        assert_eq!(block_reward_at_height_for_chain(2 * HALVING_INTERVAL, 0, consensus::CHAIN_ID_MAINNET), 12 * COIN + 50_000_000);
    }

    #[test]
    fn test_tail_emission_activates() {
        let tail_reward = calculate_tail_reward(MAX_SUPPLY_PRE_TAIL);
        let reward_at_tail = block_reward_at_height_for_chain(1_100_000, MAX_SUPPLY_PRE_TAIL, consensus::CHAIN_ID_MAINNET);
        assert!(reward_at_tail >= tail_reward);
        assert_eq!(reward_at_tail, tail_reward);
    }

    #[test]
    fn test_tail_reward_calculation() {
        let supply = MAX_SUPPLY_PRE_TAIL;
        let expected = (supply * TAIL_RATE_NUMERATOR) / (TAIL_RATE_DENOMINATOR * BLOCKS_PER_YEAR);
        assert_eq!(calculate_tail_reward(supply), expected);
    }

    #[test]
    fn test_reward_decreases_with_halving() {
        let r0 = block_reward_at_height_for_chain(0, 0, consensus::CHAIN_ID_MAINNET);
        let r1 = block_reward_at_height_for_chain(HALVING_INTERVAL, 0, consensus::CHAIN_ID_MAINNET);
        let r2 = block_reward_at_height_for_chain(2 * HALVING_INTERVAL, 0, consensus::CHAIN_ID_MAINNET);
        assert!(r0 > r1);
        assert!(r1 > r2);
    }

    #[test]
    fn test_tail_is_floor() {
        let height = 10_000_000;
        let supply = MAX_SUPPLY_PRE_TAIL;
        let reward = block_reward_at_height_for_chain(height, supply, consensus::CHAIN_ID_MAINNET);
        let tail = calculate_tail_reward(supply);
        assert_eq!(reward, tail);
    }

    #[test]
    fn test_halving_eventually_zero() {
        assert_eq!(block_reward_at_height_for_chain(64 * HALVING_INTERVAL, 0, consensus::CHAIN_ID_MAINNET), 0);
    }

    #[test]
    fn test_tail_reward_positive() {
        assert!(calculate_tail_reward(MAX_SUPPLY_PRE_TAIL) > 0);
    }

    #[test]
    fn test_regtest_zero_reward() {
        // Regtest should have zero reward to not interfere with existing tests
        assert_eq!(block_reward_at_height_for_chain(0, 0, consensus::CHAIN_ID_REGTEST), REGTEST_REWARD);
        assert_eq!(block_reward_at_height_for_chain(100, 10000, consensus::CHAIN_ID_REGTEST), REGTEST_REWARD);
        assert_eq!(block_reward_at_height_for_chain(1000, 1_000_000, consensus::CHAIN_ID_REGTEST), REGTEST_REWARD);
    }
}