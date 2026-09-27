mod common;

use common::*;
use std::sync::{Arc, RwLock};

#[test]
fn mining_on_regtest_low_difficulty() {
    let dir1 = temp_db_dir("pow_1");
    let dir2 = temp_db_dir("pow_2");
    let bc1 = Arc::new(RwLock::new(create_test_blockchain(&dir1)));
    let bc2 = Arc::new(RwLock::new(create_test_blockchain(&dir2)));

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();

    {
        let mut bc = bc1.write().unwrap();
        assert!(bc.grant_initial_balance_to_first_wallet(&addr));
    }

    adopt_from(&bc2, &bc1);
    assert_eq!(bc2.read().unwrap().chain.len(), 2);

    {
        let mut bc = bc1.write().unwrap();
        let mut tx = Transaction {
            sender: addr.clone(),
            receiver: "recipient".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: strangecoin::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: false,
        };
        sign_transaction(&mut tx, &keypairs[0].1);
        assert!(bc.add_transaction(tx).is_ok());

        let start = std::time::Instant::now();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let block = bc.mine_block(std::sync::mpsc::channel().0, &shutdown);
        let elapsed = start.elapsed();
        assert!(block.is_some(), "Block should be mined");
        assert!(
            elapsed.as_secs() < 5,
            "Mining took too long on regtest low difficulty"
        );
    }

    adopt_from(&bc2, &bc1);
    let bc2_guard = bc2.read().unwrap();
    assert_eq!(bc2_guard.chain.len(), 3);
    assert!(bc2_guard.validate_chain(), "Chain should be valid after mining");

    let _ = std::fs::remove_dir_all(&dir1);
    let _ = std::fs::remove_dir_all(&dir2);
}
