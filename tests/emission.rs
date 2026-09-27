mod common;

use common::*;

#[test]
fn emission_matches_block_reward() {
    let _dir = TestDir::new("emission");
    let mut bc = create_test_blockchain(_dir.path());

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr));

    let mut total_supply: u64 = bc.balances.values().map(|a| a.balance).sum();

    for height in 2..=11u64 {
        let expected_reward = strangecoin::economics::emission::block_reward_at_height(
            height,
            total_supply,
        );

        let mut tx = Transaction {
            sender: addr.clone(),
            receiver: "recipient".to_string(),
            amount: 0,
            nonce: (height - 1) as u64,
            chain_id: strangecoin::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: false,
        };
        sign_transaction(&mut tx, &keypairs[0].1);
        let _ = bc.add_transaction(tx);

        let before_mine: u64 = bc.balances.values().map(|a| a.balance).sum();
        mine_current(&mut bc);
        let after_mine: u64 = bc.balances.values().map(|a| a.balance).sum();

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
