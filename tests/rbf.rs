mod common;

use common::*;
use std::sync::Arc;
use std::time::Duration;
use strangecoin::consensus::MAX_RBF_REPLACEMENTS;
use strangecoin::error::StrangecoinError;
use strangecoin::events::{NodeEvent, REASON_REPLACED};
use strangecoin::serialize::{serialize_transaction_signed, txid};
use strangecoin::Transaction;

const EVENT_TIMEOUT: Duration = Duration::from_secs(2);

fn transfer(
    sender: &str,
    receiver: &str,
    amount: u64,
    nonce: u64,
    secret_key: &secp256k1::SecretKey,
) -> Transaction {
    let mut tx = Transaction {
        sender: sender.to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce,
        chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut tx, secret_key);
    tx
}

fn signed_len(tx: &Transaction) -> usize {
    serialize_transaction_signed(tx).len()
}

/// Замена с более высоким feerate принимается; каждая вытесненная tx даёт
/// ровно одно событие `TxRejected { reason: Replaced }` подписчику.
#[test]
fn rbf_replacement_emits_tx_rejected() {
    let dir = TestDir::new("rbf_replacement");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let keypairs = generate_keypairs(2);
    let sender = &keypairs[0].0;
    let secret_key = &keypairs[0].1;
    assert!(bc.grant_initial_balance_to_first_wallet(sender).unwrap());

    // Вес = 162 + receiver_len (sender = bech32 63, sig = 65). feerate = 1/вес,
    // поэтому более короткая замена — более «дорогая».
    let old_tx = transfer(sender, &"x".repeat(200), 100, 1, secret_key);
    let new_tx = transfer(sender, &"y".repeat(50), 100, 1, secret_key);
    assert!(signed_len(&new_tx) * 13 < signed_len(&old_tx) * 10);

    let rx = bc.event_bus().subscribe();

    bc.apply_tx(old_tx.clone()).expect("old tx accepted");
    assert!(
        rx.try_recv().is_err(),
        "обычная вставка события не порождает"
    );

    bc.apply_tx(new_tx.clone())
        .expect("replacement with higher feerate accepted");

    let event = rx
        .recv_timeout(EVENT_TIMEOUT)
        .expect("TxRejected event expected");
    match event {
        NodeEvent::TxRejected { txid: id, reason } => {
            assert_eq!(id, hex::encode(txid(&old_tx)));
            assert_eq!(reason, REASON_REPLACED);
        }
        other => panic!("unexpected event: {:?}", other),
    }
    assert!(
        rx.try_recv().is_err(),
        "ровно одно событие на одну вытесненную tx"
    );

    assert!(
        !bc.mempool_contains(&txid(&old_tx)),
        "исходная tx вытеснена из пула"
    );
    assert!(
        bc.mempool_contains(&txid(&new_tx)),
        "замена находится в пуле"
    );
}

/// Замена с feerate ниже старого × (1 + Δ) отклоняется; пул не меняется,
/// событий нет.
#[test]
fn rbf_replacement_with_lower_feerate_rejected() {
    let dir = TestDir::new("rbf_lower_feerate");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let keypairs = generate_keypairs(2);
    let sender = &keypairs[0].0;
    let secret_key = &keypairs[0].1;
    assert!(bc.grant_initial_balance_to_first_wallet(sender).unwrap());

    let old_tx = transfer(sender, &"x".repeat(50), 100, 1, secret_key);
    let new_tx = transfer(sender, &"y".repeat(500), 100, 1, secret_key);

    let rx = bc.event_bus().subscribe();
    bc.apply_tx(old_tx.clone()).expect("old tx accepted");

    let err = bc
        .apply_tx(new_tx.clone())
        .expect_err("replacement with lower feerate must be rejected");
    assert!(
        matches!(err, StrangecoinError::RbfFeerateTooLow { .. }),
        "unexpected error: {}",
        err
    );

    assert!(
        bc.mempool_contains(&txid(&old_tx)),
        "исходная tx осталась в пуле"
    );
    assert!(
        !bc.mempool_contains(&txid(&new_tx)),
        "отклонённая замена не попала в пул"
    );
    assert!(rx.try_recv().is_err(), "неудачная замена не анонсируется");
}

/// Анти-DoS: не более MAX_RBF_REPLACEMENTS последовательных замен одной tx.
#[test]
fn rbf_replacement_chain_is_limited() {
    let dir = TestDir::new("rbf_limit");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let keypairs = generate_keypairs(2);
    let sender = &keypairs[0].0;
    let secret_key = &keypairs[0].1;
    assert!(bc.grant_initial_balance_to_first_wallet(sender).unwrap());

    let mut prev = transfer(sender, &"a".repeat(100_000), 100, 1, secret_key);
    bc.apply_tx(prev.clone()).expect("seed tx accepted");

    // Каждая следующая замена — ровно на границе правила: len*10/13.
    let next_len = |prev: &Transaction| {
        let prev_len = signed_len(prev);
        let overhead = prev_len - prev.receiver.len();
        let len = prev_len * 10 / 13;
        assert!(len > overhead, "receiver must stay non-empty");
        len - overhead
    };

    for i in 1..=MAX_RBF_REPLACEMENTS {
        let next = transfer(
            sender,
            &"a".repeat(next_len(&prev)),
            100,
            1,
            secret_key,
        );
        bc.apply_tx(next.clone())
            .unwrap_or_else(|e| panic!("replacement {} rejected: {}", i, e));
        assert!(bc.mempool_contains(&txid(&next)));
        assert!(!bc.mempool_contains(&txid(&prev)));
        prev = next;
    }

    let overflow = transfer(
        sender,
        &"a".repeat(next_len(&prev)),
        100,
        1,
        secret_key,
    );
    let err = bc
        .apply_tx(overflow)
        .expect_err("11-я замена должна упереться в лимит");
    assert!(
        matches!(
            err,
            StrangecoinError::RbfReplacementLimit(n) if n == MAX_RBF_REPLACEMENTS
        ),
        "unexpected error: {}",
        err
    );
    assert!(
        bc.mempool_contains(&txid(&prev)),
        "лимит не трогает текущую tx"
    );
}

/// Замена конфликта вытесняет и зависимости (tx той же sender с nonce выше):
/// иначе в пуле останутся tx, чей nonce-слот уже занят новой версией.
#[test]
fn rbf_replacement_evicts_dependencies() {
    let dir = TestDir::new("rbf_deps");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let keypairs = generate_keypairs(2);
    let sender = &keypairs[0].0;
    let secret_key = &keypairs[0].1;
    assert!(bc.grant_initial_balance_to_first_wallet(sender).unwrap());

    let tx1 = transfer(sender, &"x".repeat(200), 100, 1, secret_key);
    let tx2 = transfer(sender, &"y".repeat(200), 100, 2, secret_key);
    let replacement = transfer(sender, &"z".repeat(50), 100, 1, secret_key);

    let rx = bc.event_bus().subscribe();
    bc.apply_tx(tx1.clone()).expect("tx1 accepted");
    bc.apply_tx(tx2.clone()).expect("tx2 accepted");

    bc.apply_tx(replacement.clone())
        .expect("replacement accepted");

    let expected = [txid(&tx1), txid(&tx2)];
    for want in expected {
        let event = rx
            .recv_timeout(EVENT_TIMEOUT)
            .expect("TxRejected per evicted tx");
        match event {
            NodeEvent::TxRejected { txid: id, reason } => {
                assert_eq!(id, hex::encode(want));
                assert_eq!(reason, REASON_REPLACED);
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }
    assert!(rx.try_recv().is_err(), "ровно два события");

    assert!(!bc.mempool_contains(&txid(&tx1)), "конфликт вытеснен");
    assert!(!bc.mempool_contains(&txid(&tx2)), "зависимость вытеснена");
    assert!(bc.mempool_contains(&txid(&replacement)));
    assert_eq!(bc.mempool_transactions().len(), 1);
}

/// После замены майнится новая версия; исходная tx в блоки не попадает,
/// баланс отправителя списывается ровно один раз.
#[test]
fn rbf_replacement_is_what_gets_mined() {
    let dir = TestDir::new("rbf_mine");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let keypairs = generate_keypairs(2);
    let sender = &keypairs[0].0;
    let secret_key = &keypairs[0].1;
    assert!(bc.grant_initial_balance_to_first_wallet(sender).unwrap());

    let old_tx = transfer(sender, &"x".repeat(200), 100, 1, secret_key);
    let new_tx = transfer(sender, &"y".repeat(50), 100, 1, secret_key);

    bc.apply_tx(old_tx.clone()).expect("old tx accepted");
    bc.apply_tx(new_tx.clone())
        .expect("replacement accepted");

    mine_current(&bc);

    let tip = bc.tip().expect("block mined");
    let mined: Vec<[u8; 32]> = tip.transactions.iter().map(txid).collect();
    assert!(
        mined.contains(&txid(&new_tx)),
        "замена должна попасть в блок"
    );
    assert!(
        !mined.contains(&txid(&old_tx)),
        "исходная tx не должна попасть в блок"
    );
    assert!(bc.mempool_is_empty());
    assert_eq!(
        bc.get_balance(sender),
        10000 - 100,
        "баланс списан ровно один раз"
    );
}
