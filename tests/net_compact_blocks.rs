//! S2-P22 — BIP 152 compact blocks (DoD #2: ≥ 50 % bandwidth reduction
//! vs naive full-block relay) and the `GET_BLOCK_TXN` recovery round-trip.
//!
//! Regtest (chain_id 3), in-memory `MockView` fixtures, real wire codecs —
//! the bandwidth number is measured on actual encoded bytes.

mod common;

use common::net_helpers::{coinbase_tx, make_block, padded_create_tx, transfer_tx, MockView};
use strangecoin_core::serialize::txid;
use strangecoin_net::gossip::{
    apply_block_txn, fill_up, handle_get_block_txn, CompactBlock, FillOutcome,
};
use strangecoin_net::protocol::encode_blocks;

/// A 100-body-tx block plus the coinbase.
fn block_of_100() -> (strangecoin::Block, Vec<strangecoin::Transaction>) {
    let body: Vec<strangecoin::Transaction> = (0..100u64).map(transfer_tx).collect();
    let mut txs = vec![coinbase_tx()];
    txs.extend(body.iter().cloned());
    (make_block(txs), body)
}

/// DoD #2: a ~1 MB block relays as a ~1.5 KB compact block — measured on
/// the encoded wire bytes, must be ≥ 50 % smaller (in practice ~99.8 %).
#[test]
fn compact_block_shrinks_full_block_by_at_least_50_percent() {
    let mut txs = vec![coinbase_tx()];
    for i in 0..100u64 {
        txs.push(padded_create_tx(i, 10 * 1024)); // ~10 KB init code each
    }
    let block = make_block(txs);

    let full_len = encode_blocks(std::slice::from_ref(&block))
        .expect("full block encodes")
        .len();
    let compact_len = CompactBlock::from_block(&block)
        .encode()
        .expect("compact block encodes")
        .len();

    assert!(
        full_len >= 1_000_000,
        "full block only {full_len} bytes, expected ~1 MB"
    );
    assert!(
        compact_len < 4096,
        "compact block unexpectedly large: {compact_len} bytes"
    );
    let reduction = 1.0 - (compact_len as f64 / full_len as f64);
    assert!(
        reduction >= 0.5,
        "bandwidth reduction only {:.2}% ({compact_len} vs {full_len})",
        reduction * 100.0
    );
    // DoD wording: ≥ 80 % of the body reconstructed from the compact form
    // itself — short IDs carry 8 B/tx against ~10 KB txs here.
    assert!(
        reduction >= 0.8,
        "fill-up payload share {:.2}% < 80%",
        reduction * 100.0
    );
}

/// All 100 body txs in the receiver's mempool: `fill_up` completes from
/// the mempool alone (no `GET_BLOCK_TXN` round-trip) and the reconstruction
/// matches the committed header.
#[test]
fn fills_complete_from_mempool_without_roundtrip() {
    let (block, body) = block_of_100();
    let compact = CompactBlock::from_block(&block);
    assert_eq!(compact.prefilled_txs.len(), 1, "coinbase prefilled");
    assert_eq!(compact.short_ids.len(), block.transactions.len());

    let receiver = MockView::with_mempool(body);
    match fill_up(&compact, &receiver).expect("honest compact block fills") {
        FillOutcome::Complete(rebuilt) => {
            assert_eq!(rebuilt.hash, block.hash, "header hash matches");
            assert_eq!(rebuilt.tx_root, block.tx_root, "tx_root matches");
            assert_eq!(rebuilt.transactions.len(), 101);
            for (a, b) in rebuilt.transactions.iter().zip(&block.transactions) {
                assert_eq!(txid(a), txid(b), "same txs in the same order");
            }
        }
        FillOutcome::Missing(req) => panic!("expected full reconstruction, got {req:?}"),
    }
}

/// 100-tx block with 20 txs missing from the mempool: fill-up reports the
/// gap, and the `GET_BLOCK_TXN` → `BLOCK_TXN` round-trip completes the
/// block without falling back to a full-block relay.
#[test]
fn missing_20_txs_recovered_via_get_block_txn_roundtrip() {
    let (block, all_body) = block_of_100();
    let held: Vec<strangecoin::Transaction> = all_body
        .iter()
        .enumerate()
        .filter(|(i, _)| !(40..60).contains(i))
        .map(|(_, tx)| tx.clone())
        .collect();
    assert_eq!(held.len(), 80);

    let compact = CompactBlock::from_block(&block);
    let receiver = MockView::with_mempool(held);

    let req = match fill_up(&compact, &receiver).expect("fill_up runs") {
        FillOutcome::Missing(req) => req,
        FillOutcome::Complete(_) => panic!("20 txs are missing — must not complete"),
    };
    assert_eq!(req.missing_indices.len(), 20, "exactly the 20 missing txs");

    // Broadcaster side serves the request from the full block.
    let mut broadcaster = MockView::default();
    broadcaster.insert_block(block.clone());
    let txn = handle_get_block_txn(&req, &broadcaster).expect("block is known");
    assert_eq!(txn.txs.len(), 20, "round-trip carries only the gap");

    let rebuilt =
        apply_block_txn(&compact, &req.missing_indices, &txn, &receiver).expect("apply succeeds");
    assert_eq!(rebuilt.hash, block.hash, "reconstruction matches");
    assert_eq!(rebuilt.transactions.len(), 101, "no tx lost");
}
