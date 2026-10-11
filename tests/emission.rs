mod common;

use common::*;
use strangecoin::Transaction;

#[test]
fn emission_matches_block_reward() {
    let _dir = TestDir::new("emission");
    let bc = create_test_blockchain(_dir.path());

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());

    let mut total_supply: u64 = bc.total_supply();

    for height in 2..=11u64 {
        let expected_reward = strangecoin::economics::emission::block_reward_at_height_for_chain(
            height,
            total_supply,
            strangecoin::consensus::CHAIN_ID_REGTEST,
        );

        let mut tx = Transaction {
            sender: addr.clone(),
            receiver: "recipient".to_string(),
            amount: 0,
            nonce: height - 1,
            chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
            signature: Vec::new(),
            is_coinbase: false,
        };
        sign_transaction(&mut tx, &keypairs[0].1);
        let _ = bc.apply_tx(tx);

        let before_mine: u64 = bc.total_supply();
        assert_eq!(
            before_mine, total_supply,
            "Height {}: supply before mine must equal tracked total_supply",
            height
        );
        mine_current(&bc);
        let after_mine: u64 = bc.total_supply();

        let coinbase_issued = after_mine - before_mine;
        assert_eq!(
            coinbase_issued, expected_reward,
            "Height {}: expected reward {} but got {}",
            height, expected_reward, coinbase_issued
        );

        total_supply = after_mine;
    }

    assert!(bc.validate_chain(), "Chain should be valid after 11 blocks");
}

#[test]
fn mainnet_tail_phase_reward_matches_total_supply() {
    use strangecoin::consensus::CHAIN_ID_MAINNET;
    use strangecoin::economics::emission::{
        block_reward_at_height_for_chain, BLOCKS_PER_YEAR, HALVING_INTERVAL, MAX_SUPPLY_PRE_TAIL,
        TAIL_RATE_DENOMINATOR, TAIL_RATE_NUMERATOR,
    };

    let supply_a = MAX_SUPPLY_PRE_TAIL;
    let supply_b = MAX_SUPPLY_PRE_TAIL / 2;
    let height = 64 * HALVING_INTERVAL;

    let expected_a = (supply_a * TAIL_RATE_NUMERATOR) / (TAIL_RATE_DENOMINATOR * BLOCKS_PER_YEAR);
    let expected_b = (supply_b * TAIL_RATE_NUMERATOR) / (TAIL_RATE_DENOMINATOR * BLOCKS_PER_YEAR);

    let reward_a = block_reward_at_height_for_chain(height, supply_a, CHAIN_ID_MAINNET);
    let reward_b = block_reward_at_height_for_chain(height, supply_b, CHAIN_ID_MAINNET);

    assert!(expected_a > 0, "tail reward at max supply must be positive");
    assert_eq!(reward_a, expected_a);
    assert_eq!(reward_b, expected_b);
    assert_ne!(
        reward_a, reward_b,
        "tail emission must depend on total_supply"
    );
}

#[test]
fn tail_phase_transition_at_fifth_halving() {
    use strangecoin::consensus::CHAIN_ID_MAINNET;
    use strangecoin::economics::emission::{
        block_reward_at_height_for_chain, BLOCKS_PER_YEAR, HALVING_INTERVAL, INITIAL_REWARD,
        MAX_SUPPLY_PRE_TAIL, TAIL_RATE_DENOMINATOR, TAIL_RATE_NUMERATOR,
    };

    let supply = MAX_SUPPLY_PRE_TAIL;
    let tail = (supply * TAIL_RATE_NUMERATOR) / (TAIL_RATE_DENOMINATOR * BLOCKS_PER_YEAR);
    let base_epoch_4 = INITIAL_REWARD >> 4;
    let base_epoch_5 = INITIAL_REWARD >> 5;

    assert!(
        base_epoch_4 > tail,
        "epoch 4: base {} must dominate tail {}",
        base_epoch_4,
        tail
    );
    assert!(
        base_epoch_5 < tail,
        "epoch 5: tail {} must dominate base {}",
        tail,
        base_epoch_5
    );

    let reward_epoch_4 =
        block_reward_at_height_for_chain(4 * HALVING_INTERVAL, supply, CHAIN_ID_MAINNET);
    let reward_epoch_5 =
        block_reward_at_height_for_chain(5 * HALVING_INTERVAL, supply, CHAIN_ID_MAINNET);

    assert_eq!(reward_epoch_4, base_epoch_4);
    assert_eq!(reward_epoch_5, tail);
}
