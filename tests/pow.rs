mod common;

use common::*;
use std::sync::Arc;
use strangecoin::Transaction;

#[test]
fn mining_on_regtest_low_difficulty() {
    let dir1 = TestDir::new("pow_1");
    let dir2 = TestDir::new("pow_2");
    let bc1 = Arc::new(create_test_blockchain(dir1.path()));
    let bc2 = Arc::new(create_test_blockchain(dir2.path()));

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();

    assert!(bc1.grant_initial_balance_to_first_wallet(&addr).unwrap());

    adopt_from(&bc2, &bc1);
    assert_eq!(bc2.chain_len(), 2);

    {
        let mut tx = Transaction {
            sender: addr.clone(),
            receiver: "recipient".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
            signature: Vec::new(),
            is_coinbase: false,
        };
        sign_transaction(&mut tx, &keypairs[0].1);
        assert!(bc1.apply_tx(tx).is_ok());

        let start = std::time::Instant::now();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let block = bc1.mine_block(std::sync::mpsc::channel().0, &shutdown);
        let elapsed = start.elapsed();
        assert!(block.is_some(), "Block should be mined");
        assert!(
            elapsed.as_secs() < 5,
            "Mining took too long on regtest low difficulty"
        );
    }

    adopt_from(&bc2, &bc1);
    assert_eq!(bc2.chain_len(), 3);
    assert!(bc2.validate_chain(), "Chain should be valid after mining");
}
