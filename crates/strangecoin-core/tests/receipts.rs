//! Receipt types, canonical encoding and the receipts sub-trie (S1.5-P14,
//! invariant #18).

use strangecoin_core::serialize::{compute_receipts_root, serialize_receipt};
use strangecoin_core::state::sparse_merkle::SparseMerkleTrie;
use strangecoin_core::state::{prove_receipt, receipts_subroot, verify_receipt_proof};
use strangecoin_core::vm::{ExecutionResult, Log, Receipt, TxStatus, VmError};

fn sample_log() -> Log {
    Log {
        address: "sc1contract".to_string(),
        topics: vec![[0xaa; 32], [0xbb; 32]],
        data: vec![1, 2, 3, 4],
    }
}

fn sample_receipt(txid: [u8; 32]) -> Receipt {
    Receipt {
        txid,
        gas_used: 21_000,
        logs: vec![sample_log()],
        status: TxStatus::Success,
        block_height: 5,
        tx_index: 1,
    }
}

#[test]
fn from_execution_copies_result_fields() {
    let result = ExecutionResult {
        gas_used: 777,
        logs: vec![sample_log()],
        status: TxStatus::Reverted,
        return_data: vec![9],
    };
    let receipt = Receipt::from_execution([1u8; 32], 10, 3, &result);
    assert_eq!(receipt.txid, [1u8; 32]);
    assert_eq!(receipt.gas_used, 777);
    assert_eq!(receipt.status, TxStatus::Reverted);
    assert_eq!(receipt.block_height, 10);
    assert_eq!(receipt.tx_index, 3);
    assert_eq!(receipt.logs, result.logs);
}

#[test]
fn from_vm_error_out_of_gas_burns_budget() {
    let receipt = Receipt::from_vm_error([2u8; 32], 1, 0, &VmError::OutOfGas, 50_000);
    assert_eq!(receipt.status, TxStatus::OutOfGas);
    assert_eq!(receipt.gas_used, 50_000);
    assert!(receipt.logs.is_empty());
}

#[test]
fn from_vm_error_other_failures_are_failed() {
    let err = VmError::HostFunctionError {
        name: "x".to_string(),
        reason: "y".to_string(),
    };
    let receipt = Receipt::from_vm_error([3u8; 32], 1, 2, &err, 10_000);
    assert_eq!(receipt.status, TxStatus::Failed);
    assert_eq!(receipt.gas_used, 10_000);
}

#[test]
fn tx_status_canonical_bytes_are_stable() {
    assert_eq!(TxStatus::Success.canonical_byte(), 0);
    assert_eq!(TxStatus::Reverted.canonical_byte(), 1);
    assert_eq!(TxStatus::OutOfGas.canonical_byte(), 2);
    assert_eq!(TxStatus::Failed.canonical_byte(), 3);
}

#[test]
fn canonical_encoding_is_deterministic_and_sensitive() {
    let a = sample_receipt([7u8; 32]);
    let b = sample_receipt([7u8; 32]);
    assert_eq!(serialize_receipt(&a), serialize_receipt(&b));

    let mut c = sample_receipt([7u8; 32]);
    c.status = TxStatus::Failed;
    assert_ne!(serialize_receipt(&a), serialize_receipt(&c));

    let mut d = sample_receipt([7u8; 32]);
    d.logs[0].data.push(5);
    assert_ne!(serialize_receipt(&a), serialize_receipt(&d));
}

#[test]
fn receipts_serde_roundtrip() {
    let receipt = sample_receipt([8u8; 32]);
    let json = serde_json::to_string(&receipt).expect("serialize");
    let back: Receipt = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(receipt, back);
}

#[test]
fn empty_receipts_root_is_zero() {
    assert_eq!(compute_receipts_root(&[]), [0u8; 32]);
    assert_eq!(receipts_subroot(&[]), SparseMerkleTrie::empty_root());
}

#[test]
fn receipts_root_is_deterministic_and_sensitive() {
    let receipts = vec![sample_receipt([1u8; 32]), sample_receipt([2u8; 32])];
    let root1 = compute_receipts_root(&receipts);
    let root2 = compute_receipts_root(&receipts);
    assert_eq!(root1, root2);

    let mut changed = receipts.clone();
    changed[0].gas_used += 1;
    assert_ne!(root1, compute_receipts_root(&changed));

    let mut reordered = receipts.clone();
    reordered.reverse();
    assert_eq!(root1, compute_receipts_root(&reordered));
}

#[test]
fn subroot_prove_verify_roundtrip() {
    let receipts = vec![
        sample_receipt([0x11; 32]),
        sample_receipt([0x22; 32]),
        sample_receipt([0x33; 32]),
    ];
    let root = receipts_subroot(&receipts);

    for receipt in &receipts {
        let proof = prove_receipt(&receipts, &receipt.txid).expect("proof");
        assert!(verify_receipt_proof(&root, receipt, &proof));
    }
}

#[test]
fn subroot_verify_rejects_tampered_receipt() {
    let receipts = vec![sample_receipt([0x44; 32])];
    let root = receipts_subroot(&receipts);
    let proof = prove_receipt(&receipts, &[0x44; 32]).expect("proof");

    let mut tampered = receipts[0].clone();
    tampered.gas_used = 1;
    assert!(!verify_receipt_proof(&root, &tampered, &proof));

    let mut tampered_status = receipts[0].clone();
    tampered_status.status = TxStatus::Failed;
    assert!(!verify_receipt_proof(&root, &tampered_status, &proof));
}

#[test]
fn subroot_verify_rejects_wrong_proof() {
    let a = vec![sample_receipt([0x55; 32])];
    let b = vec![sample_receipt([0x66; 32])];
    let root_a = receipts_subroot(&a);
    let proof_b = prove_receipt(&b, &[0x66; 32]).expect("proof");
    assert!(!verify_receipt_proof(&root_a, &b[0], &proof_b));
}

#[test]
fn subroot_of_single_receipt_differs_from_empty() {
    let empty = receipts_subroot(&[]);
    let single = receipts_subroot(&[sample_receipt([0x77; 32])]);
    assert_ne!(empty, single);
}
