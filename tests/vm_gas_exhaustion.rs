//! S1.5-P17: gas exhaustion — per-tx `OutOfGas` (full budget burned, no
//! value moved) and per-block reservation overflow
//! (`BlockGasLimitExceeded`, invariant #15).

#![cfg(feature = "vm-wasm")]

mod common;

use std::collections::HashMap;

use common::vm_helpers::{
    child_block, commit_block, derive, init_chain, keypair, seal, sign,
};
use strangecoin::blockchain::block_executor::{now_secs, validate_and_apply, BlockView};
use strangecoin::error::StrangecoinError;
use strangecoin::consensus::CURRENT_CONSENSUS_VERSION;
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::types::Transaction;
use strangecoin_core::vm::{TxStatus, BLOCK_GAS_LIMIT};

const INFINITE_LOOP_WAT: &str = r#"(module (func (export "main") (loop $l (br $l))))"#;

#[test]
fn call_tx_out_of_gas_burns_full_budget_and_keeps_value() {
    let (alice, alice_sk) = keypair();
    let (mut chain, mut state) = init_chain(&alice, 10_000);
    let mut codes: HashMap<String, Vec<u8>> = HashMap::new();

    // Deploy the infinite-loop contract.
    let create = sign(
        Transaction::create(
            &alice,
            INFINITE_LOOP_WAT.as_bytes().to_vec(),
            vec![],
            0,
            0,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let create_block = child_block(chain.last().unwrap(), vec![create]);
    state = commit_block(&mut chain, &state, create_block, &codes);
    let contract = derive(&alice, 0);
    codes.insert(contract.clone(), INFINITE_LOOP_WAT.as_bytes().to_vec());

    // Call with a 100k budget: the loop is halted by fuel metering.
    let call = sign(
        Transaction::call(
            &alice,
            &contract,
            vec![],
            25,
            100_000,
            1,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call]);
    state = commit_block(&mut chain, &state, call_block, &codes);

    let receipt = &state.receipts[1];
    assert_eq!(receipt.status, TxStatus::OutOfGas);
    assert_eq!(
        receipt.gas_used, 100_000,
        "OutOfGas must burn the entire tx budget"
    );
    assert!(receipt.logs.is_empty());
    assert_eq!(
        state.get_account(&contract).unwrap().balance(),
        0,
        "value must not move on OutOfGas"
    );
    assert_eq!(state.get_balance(&alice), 10_000);
}

#[test]
fn call_tx_at_block_gas_limit_burns_exactly_the_limit() {
    let (alice, alice_sk) = keypair();
    let (mut chain, mut state) = init_chain(&alice, 10_000);
    let mut codes: HashMap<String, Vec<u8>> = HashMap::new();

    let create = sign(
        Transaction::create(
            &alice,
            INFINITE_LOOP_WAT.as_bytes().to_vec(),
            vec![],
            0,
            0,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let create_block = child_block(chain.last().unwrap(), vec![create]);
    state = commit_block(&mut chain, &state, create_block, &codes);
    let contract = derive(&alice, 0);
    codes.insert(contract.clone(), INFINITE_LOOP_WAT.as_bytes().to_vec());

    // Reservation == limit is allowed; the receipt burn equals the limit
    // and still passes the post-apply receipt bound.
    let call = sign(
        Transaction::call(
            &alice,
            &contract,
            vec![],
            0,
            BLOCK_GAS_LIMIT,
            1,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call]);
    state = commit_block(&mut chain, &state, call_block, &codes);

    let receipt = &state.receipts[1];
    assert_eq!(receipt.status, TxStatus::OutOfGas);
    assert_eq!(receipt.gas_used, BLOCK_GAS_LIMIT);
}

#[test]
fn call_tx_exceeding_block_gas_limit_is_rejected() {
    let (alice, alice_sk) = keypair();
    let (chain, state) = init_chain(&alice, 10_000);

    // A single Call reserving 2M > the 1M block limit — rejected by the
    // reservation check before any state transition runs.
    let big_call = sign(
        Transaction::call(
            &alice,
            "rsc1nowhere",
            vec![],
            0,
            2_000_000,
            0,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let block = seal(child_block(chain.last().unwrap(), vec![big_call]));
    let view = BlockView::new(
        &chain,
        now_secs(),
        false,
        CURRENT_CONSENSUS_VERSION,
        CHAIN_ID_REGTEST,
    );
    let err = validate_and_apply(&state, &block, &view)
        .expect_err("reservation above the block gas limit must fail");
    assert!(
        matches!(
            err,
            StrangecoinError::BlockGasLimitExceeded { total, limit }
                if total == 2_000_000 && limit == BLOCK_GAS_LIMIT
        ),
        "expected BlockGasLimitExceeded, got {err:?}"
    );
}
