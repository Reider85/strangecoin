use crate::address::address_from_public_key;
use crate::error::CoreError;
use crate::serialize;
use crate::types::{Block, Transaction};

pub const CHAIN_ID_MAINNET: u32 = 1;
pub const CHAIN_ID_TESTNET: u32 = 2;
pub const CHAIN_ID_REGTEST: u32 = 3;

pub const CURRENT_CONSENSUS_VERSION: u32 = 1;

pub const MEDIAN_TIME_WINDOW: usize = 11;
pub const MAX_FUTURE_TIME: u64 = 2 * 60 * 60;

pub const RETARGET_INTERVAL: u64 = 2016;
pub const TARGET_BLOCK_TIME: u64 = 600;
pub const MAX_TARGET_CHANGE_FACTOR: u64 = 4;

pub type U256 = [u64; 4];

pub fn u256_from_bytes(bytes: &[u8; 32]) -> U256 {
    let mut words = [0u64; 4];
    for i in 0..4 {
        let start = i * 8;
        let end = start + 8;
        let mut word_bytes = [0u8; 8];
        word_bytes.copy_from_slice(&bytes[start..end]);
        words[i] = u64::from_be_bytes(word_bytes);
    }
    words
}

pub fn u256_from_u64(v: u64) -> U256 {
    [0, 0, 0, v]
}

pub fn u256_to_bytes(v: U256) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for i in 0..4 {
        let word_bytes = v[i].to_be_bytes();
        bytes[i * 8..(i + 1) * 8].copy_from_slice(&word_bytes);
    }
    bytes
}

pub fn u256_mul(a: U256, b: U256) -> U256 {
    let mut result = [0u128; 8];
    for i in 0..4 {
        for j in 0..4 {
            result[i + j] += a[i] as u128 * b[j] as u128;
        }
    }
    let mut carry: u128 = 0;
    for i in 0..8 {
        result[i] += carry;
        carry = result[i] >> 64;
        result[i] &= 0xFFFFFFFFFFFFFFFF;
    }
    [
        result[0] as u64,
        result[1] as u64,
        result[2] as u64,
        result[3] as u64,
    ]
}

pub fn u256_div(a: U256, b: U256) -> U256 {
    let mut remainder = [0u128; 8];
    let mut quotient = [0u64; 4];

    for i in (0..4).rev() {
        remainder[i + 4] = a[i] as u128;
    }

    for i in (0..4).rev() {
        let mut divisor = 0u128;
        for j in 0..4 {
            divisor = (divisor << 64) | b[j] as u128;
        }

        let mut dividend = 0u128;
        for j in 0..8 {
            dividend = (dividend << 64) | remainder[j];
        }

        if divisor == 0 {
            return [0, 0, 0, 0];
        }

        let q = dividend / divisor;
        quotient[i] = q as u64;

        let mut sub = 0u128;
        for j in (0..4).rev() {
            let prod = (quotient[i] as u128) * b[j] as u128 + sub;
            sub = prod >> 64;
            let diff = remainder[j + 4] - (prod & 0xFFFFFFFFFFFFFFFF);
            remainder[j + 4] = diff & 0xFFFFFFFFFFFFFFFF;
        }
    }

    quotient
}

pub fn u256_min(a: U256, b: U256) -> U256 {
    for i in (0..4).rev() {
        if a[i] < b[i] {
            return a;
        } else if a[i] > b[i] {
            return b;
        }
    }
    a
}

pub fn u256_max(a: U256, b: U256) -> U256 {
    for i in (0..4).rev() {
        if a[i] > b[i] {
            return a;
        } else if a[i] < b[i] {
            return b;
        }
    }
    a
}

pub fn u256_gt(a: U256, b: U256) -> bool {
    for i in (0..4).rev() {
        if a[i] > b[i] {
            return true;
        } else if a[i] < b[i] {
            return false;
        }
    }
    false
}

pub fn u256_le(a: U256, b: U256) -> bool {
    for i in (0..4).rev() {
        if a[i] < b[i] {
            return true;
        } else if a[i] > b[i] {
            return false;
        }
    }
    true
}

pub fn current_chain_id() -> u32 {
    CHAIN_ID_REGTEST
}

pub fn median_time_past(blocks: &[Block], current_height: u64) -> u64 {
    if current_height == 0 {
        return 0;
    }
    let start = current_height.saturating_sub(MEDIAN_TIME_WINDOW as u64);
    let mut times: Vec<u64> = blocks[start as usize..]
        .iter()
        .map(|b| b.timestamp)
        .collect();
    times.sort();
    times[times.len() / 2]
}

pub fn validate_timestamp(
    block: &Block,
    prev_blocks: &[Block],
    now: u64,
) -> Result<(), CoreError> {
    if block.index == 0 {
        return Ok(());
    }
    let mtp = median_time_past(prev_blocks, block.index);
    if block.timestamp <= mtp {
        return Err(CoreError::TimestampTooOld);
    }
    if block.timestamp > now + MAX_FUTURE_TIME {
        return Err(CoreError::TimestampInFuture);
    }
    Ok(())
}

pub fn compute_target(prev_blocks: &[Block]) -> [u8; 32] {
    if prev_blocks.len() < RETARGET_INTERVAL as usize {
        let last = prev_blocks.last().expect("at least one block");
        let target_bytes = hex::decode(&last.target).expect("valid target hex");
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&target_bytes);
        return arr;
    }
    let window = &prev_blocks[prev_blocks.len() - RETARGET_INTERVAL as usize..];
    let first = &window[0];
    let last = &window[window.len() - 1];
    let actual_time = last.timestamp.saturating_sub(first.timestamp);
    let expected_time = TARGET_BLOCK_TIME * (RETARGET_INTERVAL - 1);

    let prev_target_bytes = hex::decode(&last.target).expect("valid target hex");
    let mut prev_target_arr = [0u8; 32];
    prev_target_arr.copy_from_slice(&prev_target_bytes);

    let prev_u256 = u256_from_bytes(&prev_target_arr);
    let actual_u256 = u256_from_u64(actual_time);
    let expected_u256 = u256_from_u64(expected_time);

    let numerator = u256_mul(prev_u256, actual_u256);
    let new_target = u256_div(numerator, expected_u256);

    let max_target = u256_mul(prev_u256, u256_from_u64(MAX_TARGET_CHANGE_FACTOR));
    let min_target = u256_div(prev_u256, u256_from_u64(MAX_TARGET_CHANGE_FACTOR));
    let clamped = u256_min(u256_max(new_target, min_target), max_target);

    u256_to_bytes(clamped)
}

pub fn validate_difficulty(block: &Block) -> Result<(), CoreError> {
    let hash = serialize::block_hash(block);
    let target_bytes = hex::decode(&block.target)
        .map_err(|_| CoreError::InvalidDifficulty)?;
    let mut target_arr = [0u8; 32];
    target_arr.copy_from_slice(&target_bytes);

    let hash_u256 = u256_from_bytes(&hash);
    let target_u256 = u256_from_bytes(&target_arr);

    if u256_gt(hash_u256, target_u256) {
        return Err(CoreError::InvalidDifficulty);
    }
    Ok(())
}

pub fn recover_pubkey_from_sig(
    signature: &[u8],
    message: &[u8],
) -> Result<secp256k1::PublicKey, CoreError> {
    if signature.len() != 65 {
        return Err(CoreError::InvalidSignature);
    }
    let secp = secp256k1::Secp256k1::new();
    let msg_hash = blake3::hash(message);
    let msg = secp256k1::Message::from_digest_slice(msg_hash.as_bytes())
        .map_err(|_| CoreError::InvalidSignature)?;
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(&signature[..64]);
    let rec_id = secp256k1::ecdsa::RecoveryId::from_i32(signature[64] as i32)
        .map_err(|_| CoreError::InvalidSignature)?;
    let sig = secp256k1::ecdsa::RecoverableSignature::from_compact(&sig_bytes, rec_id)
        .map_err(|_| CoreError::InvalidSignature)?;
    let recovered_pk = secp
        .recover_ecdsa(&msg, &sig)
        .map_err(|_| CoreError::InvalidSignature)?;
    Ok(recovered_pk)
}

pub fn verify_transaction(tx: &Transaction) -> Result<(), CoreError> {
    if tx.is_coinbase {
        return Ok(());
    }
    let pk = recover_pubkey_from_sig(&tx.signature, &serialize::serialize_transaction(tx))?;
    if address_from_public_key(&pk) != tx.sender {
        return Err(CoreError::InvalidSignature);
    }
    Ok(())
}

pub const EXPECTED_GENESIS_HASH: [u8; 32] = [
    0x56, 0x3c, 0x9e, 0x51, 0x34, 0x23, 0x44, 0xc0, 0x1b, 0x93, 0x18, 0x53, 0xc4, 0x9e, 0x22,
    0x74, 0xb6, 0xfb, 0xd2, 0xde, 0x99, 0xed, 0x50, 0xb8, 0xd6, 0xd3, 0xbd, 0x7a, 0x7e, 0x65,
    0xab, 0x50,
];

pub fn validate_tx_root(block: &Block) -> Result<(), CoreError> {
    let computed = serialize::compute_tx_root(&block.transactions);
    if computed != block.tx_root {
        return Err(CoreError::TxRootMismatch {
            expected: computed,
            got: block.tx_root,
        });
    }
    Ok(())
}

pub fn is_regtest(network_id: u32) -> bool {
    network_id == CHAIN_ID_REGTEST
}

pub fn work_from_target(target: &[u8; 32]) -> U256 {
    let target_u256 = u256_from_bytes(target);
    if target_u256 == [0, 0, 0, 0] {
        return [0, 0, 0, 0];
    }
    let max_u256: U256 = [u64::MAX, u64::MAX, u64::MAX, u64::MAX];
    u256_div(max_u256, target_u256)
}

pub fn cumulative_work(chain: &[Block]) -> U256 {
    let mut total: U256 = [0, 0, 0, 0];
    for block in chain {
        let target_bytes = hex::decode(&block.target).expect("valid target hex");
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&target_bytes);
        let w = work_from_target(&arr);
        total = u256_add(total, w);
    }
    total
}

pub fn u256_add(a: U256, b: U256) -> U256 {
    let mut result = [0u64; 4];
    let mut carry: u64 = 0;
    for i in 0..4 {
        let sum = a[i] as u128 + b[i] as u128 + carry as u128;
        result[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }
    result
}
