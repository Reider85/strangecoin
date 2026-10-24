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

/// Минимальный прирост feerate для RBF-замены, в базисных пунктах (1 bps =
/// 0.01%). Замена допустима только если новый feerate ≥ старый × (1 + Δ/10000).
///
/// Δ = 30%: ниже ~25% правило не отличает замену от обычного дубля по nonce
/// с более короткой (а значит, более «дорогой» по прокси-метрике) сериализацией
/// — тест double_spend (D01) обязан оставаться зелёным.
pub const RBF_MIN_DELTA_BPS: u64 = 3000;
pub const RBF_BPS_DENOMINATOR: u64 = 10_000;
pub const MAX_RBF_REPLACEMENTS: u32 = 10;

pub type U256 = [u64; 4];

/// Limb order is little-endian: `words[0]` is the least-significant 64 bits —
/// the layout `u256_mul`/`u256_div`/`u256_gt` operate on. The byte slice is
/// standard big-endian, so the first 8 bytes map to `words[3]` (BUG-S1-016:
/// converters previously stored limbs in BE order, which made the arithmetic
/// helpers compare and scale a limb-reversed value — the retarget clamp in
/// `compute_target` never engaged and `validate_difficulty` did not implement
/// a numeric `hash <= target`).
pub fn u256_from_bytes(bytes: &[u8; 32]) -> U256 {
    let mut words = [0u64; 4];
    let (chunks, _) = bytes.as_chunks::<8>();
    for (i, chunk) in chunks.iter().enumerate() {
        words[3 - i] = u64::from_be_bytes(*chunk);
    }
    words
}

pub fn u256_from_u64(v: u64) -> U256 {
    [v, 0, 0, 0]
}

pub fn u256_to_bytes(v: U256) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for (i, word) in v.iter().enumerate() {
        let word_bytes = word.to_be_bytes();
        bytes[(3 - i) * 8..(4 - i) * 8].copy_from_slice(&word_bytes);
    }
    bytes
}

pub fn u256_mul(a: U256, b: U256) -> U256 {
    // Schoolbook multiplication: 4x4 limbs into a 8-limb intermediate,
    // keeping only the low 256 bits. Each step fits in u128 because
    // (2^64-1)^2 + (2^64-1) + (2^64-1) == 2^128 - 1.
    let mut r = [0u64; 8];
    for (i, &ai) in a.iter().enumerate() {
        if ai == 0 {
            continue;
        }
        let mut carry: u128 = 0;
        for (j, &bj) in b.iter().enumerate() {
            let idx = i + j;
            let cur = (ai as u128) * (bj as u128) + r[idx] as u128 + carry;
            r[idx] = cur as u64;
            carry = cur >> 64;
        }
        let mut k = i + 4;
        while carry != 0 && k < 8 {
            let cur = r[k] as u128 + carry;
            r[k] = cur as u64;
            carry = cur >> 64;
            k += 1;
        }
    }
    [r[0], r[1], r[2], r[3]]
}

fn u256_shl1(v: &mut U256) -> u64 {
    let mut carry = 0u64;
    for limb in v.iter_mut() {
        let next = *limb >> 63;
        *limb = (*limb << 1) | carry;
        carry = next;
    }
    carry
}

fn u256_sub_assign(a: &mut U256, b: &U256) {
    let mut borrow = 0u64;
    for i in 0..4 {
        let (d1, b1) = a[i].overflowing_sub(b[i]);
        let (d2, b2) = d1.overflowing_sub(borrow);
        a[i] = d2;
        borrow = (b1 as u64) + (b2 as u64);
    }
}

/// Truncating 256/256 division (restoring binary long division).
/// Returns `[0; 4]` for a zero divisor, matching the previous behaviour.
pub fn u256_div(a: U256, b: U256) -> U256 {
    if b == [0, 0, 0, 0] {
        return [0, 0, 0, 0];
    }
    if u256_gt(b, a) {
        return [0, 0, 0, 0];
    }
    let mut quotient = [0u64; 4];
    let mut remainder = [0u64; 4];
    for bit in (0..256usize).rev() {
        // The previous remainder is strictly < b, so after the shift the value
        // is < 2b and a single conditional subtraction is enough. When the top
        // bit was shifted out the true value is 2^256 + remainder, which is
        // still >= b, and wrapping subtraction yields exactly value - b.
        let overflowed = u256_shl1(&mut remainder) == 1;
        if (a[bit / 64] >> (bit % 64)) & 1 == 1 {
            remainder[0] |= 1;
        }
        if overflowed || !u256_gt(b, remainder) {
            u256_sub_assign(&mut remainder, &b);
            quotient[bit / 64] |= 1u64 << (bit % 64);
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

/// `a * b` saturating at `U256::MAX` instead of silently wrapping.
/// The retarget ceiling `prev_target * 4` must never wrap: a wrapped ceiling
/// lands below the `prev_target / 4` floor and makes the clamp in
/// [`compute_target`] return a value below the minimum (BUG-S1-016).
pub fn u256_mul_sat(a: U256, b: U256) -> U256 {
    if a == [0, 0, 0, 0] || b == [0, 0, 0, 0] {
        return [0, 0, 0, 0];
    }
    let product = u256_mul(a, b);
    // Overflow check: (a*b)/b must recover a exactly.
    if u256_div(product, b) == a {
        product
    } else {
        [u64::MAX; 4]
    }
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

pub fn validate_timestamp(block: &Block, prev_blocks: &[Block], now: u64) -> Result<(), CoreError> {
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
    let min_target = u256_div(prev_u256, u256_from_u64(MAX_TARGET_CHANGE_FACTOR));
    // Ceiling saturates instead of wrapping (u256_mul_sat): a wrapped
    // `prev * 4` would sit below the `prev / 4` floor and the clamp could
    // return a target below the minimum (BUG-S1-016).
    let max_target = u256_mul_sat(prev_u256, u256_from_u64(MAX_TARGET_CHANGE_FACTOR));

    // Decide the clamp bounds from the u64 ratio before touching the u256
    // multiply: outside [1/4, 4] the answer is a bound, and returning early
    // keeps near-max targets from wrapping the numerator (BUG-S1-016).
    if actual_time >= expected_time.saturating_mul(MAX_TARGET_CHANGE_FACTOR) {
        return u256_to_bytes(max_target);
    }
    if actual_time.saturating_mul(MAX_TARGET_CHANGE_FACTOR) <= expected_time {
        return u256_to_bytes(min_target);
    }

    let numerator = u256_mul(prev_u256, u256_from_u64(actual_time));
    let new_target = u256_div(numerator, u256_from_u64(expected_time));
    let clamped = u256_min(u256_max(new_target, min_target), max_target);

    u256_to_bytes(clamped)
}

pub fn validate_difficulty(block: &Block) -> Result<(), CoreError> {
    let hash = serialize::block_hash(block);
    let target_bytes = hex::decode(&block.target).map_err(|_| CoreError::InvalidDifficulty)?;
    let mut target_arr = [0u8; 32];
    target_arr.copy_from_slice(&target_bytes);

    let hash_u256 = u256_from_bytes(&hash);
    let target_u256 = u256_from_bytes(&target_arr);

    if u256_gt(hash_u256, target_u256) {
        return Err(CoreError::InvalidDifficulty);
    }
    Ok(())
}

/// Header-only PoW check (S1-P16): the header's own hash must meet its
/// declared target, and a non-empty declared `hash` field must match the
/// recomputed one. Body rules (retarget schedule, tx_root, signatures) are
/// deliberately NOT checked here — headers only lay out the route.
pub fn validate_header_pow(header: &crate::types::BlockHeader) -> Result<(), CoreError> {
    let computed = serialize::header_hash(header);
    if !header.hash.is_empty() && header.hash != hex::encode(computed) {
        return Err(CoreError::HeaderHashMismatch {
            expected: hex::encode(computed),
            got: header.hash.clone(),
        });
    }
    let target_bytes = hex::decode(&header.target).map_err(|_| CoreError::InvalidDifficulty)?;
    if target_bytes.len() != 32 {
        return Err(CoreError::InvalidDifficulty);
    }
    let mut target_arr = [0u8; 32];
    target_arr.copy_from_slice(&target_bytes);
    if u256_gt(u256_from_bytes(&computed), u256_from_bytes(&target_arr)) {
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

/// Nonce-инвариант (ARCHITECT3 §5 №11): транзакция отправителя должна иметь
/// `nonce == account.nonce + 1`. Чистая функция — единственное определение
/// правила для mempool и будущих state-transition проверок (BUG-S0-025).
pub fn validate_nonce(tx_nonce: u64, account_nonce: u64) -> Result<(), CoreError> {
    let expected = account_nonce
        .checked_add(1)
        .ok_or(CoreError::StateOverflow)?;
    if tx_nonce != expected {
        return Err(CoreError::InvalidNonce {
            expected,
            got: tx_nonce,
        });
    }
    Ok(())
}

pub fn verify_transaction(tx: &Transaction) -> Result<(), CoreError> {
    if tx.is_coinbase() {
        return Ok(());
    }
    match tx {
        Transaction::Create(create) => {
            if create.init_code.is_empty() {
                return Err(CoreError::InvalidContractDeploy {
                    reason: "init_code is empty".to_string(),
                });
            }
            if create.init_code.len() > crate::vm::limits::MAX_CODE_SIZE {
                return Err(CoreError::InvalidContractDeploy {
                    reason: format!(
                        "init_code exceeds max code size of {} bytes",
                        crate::vm::limits::MAX_CODE_SIZE
                    ),
                });
            }
            // S1.5-FBP04: the constructor budget must fit the block limit.
            // `0` stays legal — legacy deploys of modules without a
            // `constructor` export reserve and burn nothing.
            if create.gas_limit > crate::vm::BLOCK_GAS_LIMIT {
                return Err(CoreError::InvalidGasLimit {
                    reason: format!(
                        "create gas_limit {} exceeds block gas limit {}",
                        create.gas_limit,
                        crate::vm::BLOCK_GAS_LIMIT
                    ),
                });
            }
        }
        Transaction::Call(call) => {
            if call.gas_limit == 0 {
                return Err(CoreError::InvalidGasLimit {
                    reason: "gas_limit must be > 0".to_string(),
                });
            }
        }
        Transaction::Transfer(_) => {}
    }
    let pk = recover_pubkey_from_sig(tx.signature(), &serialize::serialize_transaction(tx))?;
    let (sender_pk, _net) = crate::address::decode_address(tx.sender())?;
    if sender_pk != pk {
        return Err(CoreError::InvalidSignature);
    }
    Ok(())
}

/// Hash of the mainnet genesis block built from `genesis.json`.
/// S1.5-P08: regenerated after the FORMAT_VERSION 5 transaction encoding bump
/// (Transaction enum: transfer/create/call; fee in the signed payload).
/// S1.5-FBP04: regenerated after the FORMAT_VERSION 6 bump (Create carries
/// `constructor_params` + `gas_limit`; the shared version byte moves
/// headers, so the genesis hash moves with it).
/// S1.5-FBP07: regenerated after the FORMAT_VERSION 7 bump (header gains
/// `receipts_root`, ADR-0016).
pub const EXPECTED_GENESIS_HASH: [u8; 32] = [
    0x0d, 0x40, 0x0a, 0x4c, 0x50, 0x61, 0x4e, 0x50, 0xee, 0x4b, 0x33, 0x61, 0x7f, 0x90, 0xe8, 0x53,
    0x37, 0xbb, 0x3b, 0x18, 0xc8, 0x8c, 0xe7, 0xb5, 0xb9, 0x2a, 0x9d, 0x89, 0x74, 0x5a, 0x83, 0xcc,
];

/// Hash of the testnet genesis block built from `genesis-testnet.json`
/// (BUG-S1-004). The testnet initial holder may reuse the burned testnet
/// pubkey (SCIP-0001 §2: burned-key material is testnet/regtest-only).
/// S1.5-FBP07: regenerated after the FORMAT_VERSION 7 bump (header gains
/// `receipts_root`, ADR-0016).
pub const EXPECTED_TESTNET_GENESIS_HASH: [u8; 32] = [
    0x71, 0x0d, 0x83, 0xd5, 0xc9, 0x2c, 0x4e, 0x57, 0x40, 0x00, 0x28, 0xb1, 0x5d, 0x97, 0xe8, 0x25,
    0xd9, 0x1d, 0x92, 0x81, 0x82, 0x4d, 0x54, 0x72, 0xa1, 0x10, 0xe4, 0xd0, 0x6d, 0x24, 0xb2, 0x70,
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

/// Header-only cumulative work (S1-P16): same rule as [`cumulative_work`],
/// applied to headers — lets fork choice run before any body is downloaded.
pub fn cumulative_work_headers(headers: &[crate::types::BlockHeader]) -> U256 {
    let mut total: U256 = [0, 0, 0, 0];
    for header in headers {
        let target_bytes = hex::decode(&header.target).expect("valid target hex");
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
