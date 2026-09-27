mod common;

use common::*;
use strangecoin::error::StrangecoinError;

#[test]
fn grant_block_rejected_when_disabled() {
    let dir = TestDir::new("grant_disabled");
    let mut bc = create_test_blockchain(dir.path());
    bc.allow_grant_blocks = false;

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();

    let result = bc.grant_initial_balance_to_first_wallet(&addr);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StrangecoinError::GrantBlocksDisabled));
    assert_eq!(bc.chain.len(), 1, "Chain should still have only genesis block");
}

#[test]
fn grant_block_accepted_when_enabled() {
    let dir = TestDir::new("grant_enabled");
    let mut bc = create_test_blockchain(dir.path());
    bc.allow_grant_blocks = true;

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();

    let result = bc.grant_initial_balance_to_first_wallet(&addr);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), true);
    assert_eq!(bc.chain.len(), 2, "Chain should have genesis + grant block");
    let balance = bc.balances.get(&addr).map(|a| a.balance).unwrap_or(0);
    assert_eq!(balance, 10000, "Wallet should have 10000 from genesis");
}

#[test]
fn block1_rejects_over_emission_when_disabled() {
    let dir = TestDir::new("block1_emission");
    let mut bc = create_test_blockchain(dir.path());
    bc.allow_grant_blocks = false;

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();

    // Manually create a grant block with excessive coinbase (10000 > block_reward_at_height(1, ...))
    use strangecoin::Transaction;
    use strangecoin::Block;

    let previous_block = bc.chain.last().unwrap().clone();
    let genesis_balance = bc.balances.get("initial_wallet_address").map(|a| a.balance).unwrap_or(0);

    let transaction = Transaction {
        sender: "initial_wallet_address".to_string(),
        receiver: addr.clone(),
        amount: genesis_balance,
        nonce: 0,
        chain_id: strangecoin::consensus::current_chain_id(),
        signature: Vec::new(),
        is_coinbase: true,
    };
    let mut block = Block {
        index: previous_block.index + 1,
        timestamp: 0,
        transactions: vec![transaction],
        previous_hash: previous_block.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: previous_block.target.clone(),
    };
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    bc.chain.push(block);

    // validate_chain should reject: block 1 coinbase exceeds emission schedule
    assert!(!bc.validate_chain(), "Chain with over-emission grant block should be rejected when allow_grant_blocks = false");
}

#[test]
fn magic_string_sender_rejected_in_normal_block() {
    let dir = TestDir::new("magic_string");
    let mut bc = create_test_blockchain(dir.path());
    bc.allow_grant_blocks = true;

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());

    use strangecoin::Transaction;
    use strangecoin::Block;

    // Create block 2 with sender="genesis" (should be rejected in validate_chain)
    let previous_block = bc.chain.last().unwrap().clone();
    let fake_tx = Transaction {
        sender: "genesis".to_string(),
        receiver: addr.clone(),
        amount: 9999,
        nonce: 0,
        chain_id: strangecoin::consensus::current_chain_id(),
        signature: Vec::new(),
        is_coinbase: false,
    };
    let mut block = Block {
        index: previous_block.index + 1,
        timestamp: previous_block.timestamp + 1,
        transactions: vec![fake_tx],
        previous_hash: previous_block.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: previous_block.target.clone(),
    };
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    bc.chain.push(block);

    // validate_chain should reject: block 2 with sender="genesis" has no balance
    assert!(!bc.validate_chain(), "Chain with magic string sender in non-genesis block should be rejected");
}
