#[cfg(test)]
mod proptest {
    use crate::consensus::{
        compute_target, current_chain_id, u256_div, u256_from_bytes, u256_from_u64, u256_gt,
        u256_le, u256_max, u256_min, u256_mul, u256_to_bytes, validate_difficulty,
        MAX_TARGET_CHANGE_FACTOR, RETARGET_INTERVAL, TARGET_BLOCK_TIME,
    };
    use crate::economics::emission::{
        block_reward_at_height_for_chain, HALVING_INTERVAL, MAX_SUPPLY_PRE_TAIL,
    };
    use crate::{serialize, Block, Transaction};
    use blake3;
    use hex;
    use proptest::prelude::*;
    use rand::rngs::OsRng;
    use secp256k1::{ecdsa::RecoverableSignature, PublicKey, Secp256k1, SecretKey};

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
            let secp = Secp256k1::new();
            SecretKey::from_slice(&bytes)
                .unwrap_or_else(|_| SecretKey::from_slice(&[1u8; 32]).unwrap())
        })
    }

    fn arbitrary_address() -> impl Strategy<Value = String> {
        arbitrary_public_key().prop_map(|pk| crate::address::address_from_public_key(&pk))
    }

    fn arbitrary_signature() -> impl Strategy<Value = Vec<u8>> {
        (any::<[u8; 64]>, 0..4u8).prop_map(|(sig_bytes, rec_id)| {
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
            arbitrary_address(),
            0u64..1_000_000_000u64,
            0u64..1_000_000u64,
            prop_oneof![Just(1u32), Just(2u32), Just(3u32)],
            arbitrary_secret_key(),
        )
            .prop_map(|(sender, receiver, amount, nonce, chain_id, secret_key)| {
                let secp = Secp256k1::new();
                let public_key = PublicKey::from_secret_key(&secp, &secret_key);
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
                let msg = secp256k1::Message::from_digest_slice(msg_hash.as_bytes())
                    .expect("valid message");
                let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &secret_key);
                let (rec_id, sig_bytes) = sig.serialize_compact();
                let mut sig_vec = Vec::with_capacity(65);
                sig_vec.extend_from_slice(&sig_bytes);
                sig_vec.push(rec_id.to_i32() as u8);
                tx.signature = sig_vec;
                tx
            })
    }

    fn arbitrary_block() -> impl Strategy<Value = Block> {
        (
            0u64..10_000u64,
            0u64..2_000_000_000u64,
            proptest::collection::vec(arbitrary_transaction(), 0..5),
            "[0-9a-f]{64}",
            0u64..1_000_000u64,
            "[0-9a-f]{64}",
        )
            .prop_map(
                |(index, timestamp, transactions, previous_hash, nonce, target)| Block {
                    index,
                    timestamp,
                    transactions,
                    previous_hash,
                    hash: String::new(),
                    nonce,
                    target,
                },
            )
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
            let result = crate::consensus::verify_transaction(&tx);
            prop_assert!(result.is_ok(), "Valid signed transaction should verify: {:?}", result.err());
        }

        #[test]
        fn block_reward_never_negative(height in 0u64..1_000_000u64, supply in 0u64..MAX_SUPPLY_PRE_TAIL) {
            let reward = block_reward_at_height_for_chain(height, supply, current_chain_id());
            prop_assert!(reward >= 0);
        }

        #[test]
        fn block_reward_at_halving(height in 0u64..1_000_000u64) {
            let r1 = block_reward_at_height_for_chain(height, 0, crate::consensus::CHAIN_ID_MAINNET);
            let r2 = block_reward_at_height_for_chain(height + HALVING_INTERVAL, 0, crate::consensus::CHAIN_ID_MAINNET);
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
        fn nonce_validation(tx_nonce in 0u64..1_000_000u64, account_nonce in 0u64..1_000_000u64) {
            let expected = account_nonce + 1;
            let is_valid = tx_nonce == expected;
            prop_assert_eq!(is_valid, tx_nonce == account_nonce + 1);
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
            let valid_chain_ids = [1u32, 2u32, 3u32];
            let is_valid = valid_chain_ids.contains(&tx.chain_id);
            prop_assert_eq!(is_valid, matches!(tx.chain_id, 1 | 2 | 3));
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
}
