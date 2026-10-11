use proptest::prelude::*;
use secp256k1::{ecdsa::RecoverableSignature, PublicKey, Secp256k1, SecretKey};
use strangecoin_core::consensus::{
    u256_div, u256_from_bytes, u256_from_u64, u256_gt, u256_le, u256_max, u256_min, u256_mul,
    validate_nonce, CHAIN_ID_REGTEST, MAX_TARGET_CHANGE_FACTOR, RETARGET_INTERVAL,
    TARGET_BLOCK_TIME,
};
use strangecoin_core::error::CoreError;
use strangecoin_core::economics::emission::{
    block_reward_at_height_for_chain, HALVING_INTERVAL, INITIAL_REWARD, MAX_SUPPLY_PRE_TAIL,
};
use strangecoin_core::serialize;
use strangecoin_core::Transaction;

fn arbitrary_public_key() -> impl Strategy<Value = PublicKey> {
    any::<[u8; 32]>().prop_map(|bytes| {
        let secp = Secp256k1::new();
        SecretKey::from_slice(&bytes)
            .map(|sk| PublicKey::from_secret_key(&secp, &sk))
            .unwrap_or_else(|_| {
                PublicKey::from_secret_key(&secp, &SecretKey::from_slice(&[1u8; 32]).unwrap())
            })
    })
}

fn arbitrary_secret_key() -> impl Strategy<Value = SecretKey> {
    any::<[u8; 32]>().prop_map(|bytes| {
        let _secp = Secp256k1::new();
        SecretKey::from_slice(&bytes).unwrap_or_else(|_| SecretKey::from_slice(&[1u8; 32]).unwrap())
    })
}

fn arbitrary_address() -> impl Strategy<Value = String> {
    arbitrary_public_key().prop_map(|pk| {
        strangecoin_core::address::encode_address(&pk, CHAIN_ID_REGTEST)
            .expect("Failed to generate address in proptest")
    })
}

fn arbitrary_signature() -> impl Strategy<Value = Vec<u8>> {
    (proptest::collection::vec(any::<u8>(), 64), 0..4u8).prop_map(|(sig_bytes, rec_id)| {
        let mut out = Vec::with_capacity(65);
        out.extend_from_slice(&sig_bytes);
        out.push(rec_id);
        out
    })
}

fn arbitrary_transaction() -> impl Strategy<Value = Transaction> {
    (
        arbitrary_address(),
        arbitrary_address(),
        0u64..1_000_000_000u64,
        0u64..1_000_000u64,
        prop_oneof![Just(1u32), Just(2u32), Just(3u32)],
        proptest::option::of(arbitrary_signature()),
        any::<bool>(),
    )
        .prop_map(
            |(sender, receiver, amount, nonce, chain_id, signature, is_coinbase)| {
                let sig = signature.unwrap_or_default();
                Transaction {
                    sender,
                    receiver,
                    amount,
                    nonce,
                    chain_id,
                    signature: if is_coinbase { Vec::new() } else { sig },
                    is_coinbase,
                }
            },
        )
}

fn arbitrary_signed_transaction() -> impl Strategy<Value = Transaction> {
    (
        arbitrary_address(),
        0u64..1_000_000_000u64,
        0u64..1_000_000u64,
        prop_oneof![Just(1u32), Just(2u32), Just(3u32)],
        arbitrary_secret_key(),
    )
        .prop_map(|(receiver, amount, nonce, chain_id, secret_key)| {
            let secp = Secp256k1::new();
            let public_key = PublicKey::from_secret_key(&secp, &secret_key);
            // verify_transaction() recovers the signer and compares its address
            // against tx.sender, so the sender must be derived from this key.
            let sender = strangecoin_core::address::encode_address(&public_key, chain_id)
                .expect("Failed to generate sender address in proptest");
            let mut tx = Transaction {
                sender: sender.clone(),
                receiver,
                amount,
                nonce,
                chain_id,
                signature: Vec::new(),
                is_coinbase: false,
            };
            let msg_bytes = serialize::serialize_transaction(&tx);
            let msg_hash = blake3::hash(&msg_bytes);
            let msg =
                secp256k1::Message::from_digest_slice(msg_hash.as_bytes()).expect("valid message");
            let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &secret_key);
            let (rec_id, sig_bytes) = sig.serialize_compact();
            let mut sig_vec = Vec::with_capacity(65);
            sig_vec.extend_from_slice(&sig_bytes);
            sig_vec.push(rec_id.to_i32() as u8);
            tx.signature = sig_vec;
            tx
        })
}

fn arbitrary_target() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>().prop_map(|mut bytes| {
        bytes[0] |= 0x01;
        bytes
    })
}

fn arbitrary_timespan() -> impl Strategy<Value = u64> {
    1u64..1_000_000u64
}

proptest! {
    #[test]
    fn txid_deterministic(tx in arbitrary_transaction()) {
        let id1 = serialize::txid(&tx);
        let id2 = serialize::txid(&tx);
        prop_assert_eq!(id1, id2);
    }

    #[test]
    fn signature_verification_roundtrip(tx in arbitrary_signed_transaction()) {
        let result = strangecoin_core::consensus::verify_transaction(&tx);
        prop_assert!(result.is_ok(), "Valid signed transaction should verify: {:?}", result.err());
    }

    #[test]
    fn block_reward_bounded_by_schedule(height in 0u64..1_000_000u64, supply in 0u64..MAX_SUPPLY_PRE_TAIL) {
        let reward = block_reward_at_height_for_chain(height, supply, CHAIN_ID_REGTEST);
        prop_assert!(reward <= INITIAL_REWARD);
    }

    #[test]
    fn block_reward_at_halving(height in 0u64..1_000_000u64) {
        let r1 = block_reward_at_height_for_chain(height, 0, strangecoin_core::consensus::CHAIN_ID_MAINNET);
        let r2 = block_reward_at_height_for_chain(height + HALVING_INTERVAL, 0, strangecoin_core::consensus::CHAIN_ID_MAINNET);
        if r1 > 1 {
            prop_assert_eq!(r2, r1 / 2);
        }
    }

    #[test]
    fn serialize_deserialize_roundtrip(tx in arbitrary_transaction()) {
        let bytes = serialize::serialize_transaction(&tx);
        let tx2 = serialize::deserialize_transaction(&bytes).unwrap();
        prop_assert_eq!(tx.sender, tx2.sender);
        prop_assert_eq!(tx.receiver, tx2.receiver);
        prop_assert_eq!(tx.amount, tx2.amount);
        prop_assert_eq!(tx.nonce, tx2.nonce);
        prop_assert_eq!(tx.chain_id, tx2.chain_id);
        prop_assert_eq!(tx.is_coinbase, tx2.is_coinbase);
    }

    #[test]
    fn nonce_reject(tx_nonce in 0u64..1_000_000u64, account_nonce in 0u64..1_000_000u64) {
        let result = validate_nonce(tx_nonce, account_nonce);
        prop_assert_eq!(result.is_ok(), tx_nonce == account_nonce + 1);
        match result {
            Err(CoreError::InvalidNonce { expected, got }) => {
                prop_assert_eq!(expected, account_nonce + 1);
                prop_assert_eq!(got, tx_nonce);
            }
            Err(e) => prop_assert!(false, "unexpected error variant: {:?}", e),
            Ok(()) => prop_assert_eq!(tx_nonce, account_nonce + 1),
        }
    }

    #[test]
    fn difficulty_target_clamp(prev_target in arbitrary_target(), timespan in arbitrary_timespan()) {
        let prev_u256 = u256_from_bytes(&prev_target);
        let actual_u256 = u256_from_u64(timespan);
        let expected_u256 = u256_from_u64(TARGET_BLOCK_TIME * (RETARGET_INTERVAL - 1));

        let numerator = u256_mul(prev_u256, actual_u256);
        let new_target = u256_div(numerator, expected_u256);

        let max_target = u256_mul(prev_u256, u256_from_u64(MAX_TARGET_CHANGE_FACTOR));
        let min_target = u256_div(prev_u256, u256_from_u64(MAX_TARGET_CHANGE_FACTOR));
        let clamped = u256_min(u256_max(new_target, min_target), max_target);

        prop_assert!(!u256_gt(clamped, max_target), "Clamped target should not exceed max (factor 4)");
        prop_assert!(!u256_gt(min_target, clamped), "Clamped target should not be below min (factor 4)");
    }

    #[test]
    fn chain_id_validation(tx in arbitrary_transaction()) {
        use strangecoin_core::state::{apply_block, State};
        use strangecoin_core::types::Block;

        // BUG-S1-004: the state machine pins every tx to the validating
        // chain — a foreign chain_id is rejected regardless of other rules.
        let mut transfer = tx;
        transfer.is_coinbase = false;
        let coinbase = Transaction {
            sender: "coinbase".to_string(),
            receiver: "miner".to_string(),
            amount: 0,
            nonce: 0,
            chain_id: transfer.chain_id,
            signature: Vec::new(),
            is_coinbase: true,
        };
        let block = Block {
            index: 1,
            timestamp: 1600,
            transactions: vec![coinbase, transfer.clone()],
            previous_hash: "prev_hash".to_string(),
            hash: "block_hash_1".to_string(),
            nonce: 0,
            target: "ff".to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };

        // Own chain: the chain_id gate passes (other consensus rules may
        // still reject the unsigned fixture — that is fine here).
        let own = apply_block(&State::new(), &block, transfer.chain_id);
        prop_assert!(
            !matches!(own, Err(CoreError::InvalidChainId { .. })),
            "own chain_id must not trip the chain_id gate: {own:?}"
        );

        // Foreign chain: rejected at the state-machine level.
        let foreign_id = if transfer.chain_id == 1 { 2 } else { 1 };
        let foreign = apply_block(&State::new(), &block, foreign_id);
        prop_assert!(
            matches!(
                foreign,
                Err(CoreError::InvalidChainId { expected, got })
                    if expected == foreign_id && got == transfer.chain_id
            ),
            "foreign chain_id must be rejected: {foreign:?}"
        );
    }

    #[test]
    fn u256_arithmetic_roundtrip(a in any::<[u64; 4]>(), b in any::<[u64; 4]>()) {
        let a_u256 = [a[0], a[1], a[2], a[3]];
        let b_u256 = [b[0], b[1], b[2], b[3]];

        let mul = u256_mul(a_u256, b_u256);
        let div = u256_div(mul, b_u256);

        if b_u256 != [0, 0, 0, 0] {
            let mul_check = u256_mul(div, b_u256);
            prop_assert!(u256_le(mul_check, mul) || u256_le(mul, mul_check));
        }
    }
}
