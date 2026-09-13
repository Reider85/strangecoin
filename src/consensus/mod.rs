pub const CHAIN_ID_MAINNET: u32 = 1;
pub const CHAIN_ID_TESTNET: u32 = 2;
pub const CHAIN_ID_REGTEST: u32 = 3;

pub const MEDIAN_TIME_WINDOW: usize = 11;
pub const MAX_FUTURE_TIME: u64 = 2 * 60 * 60; // 2 hours

pub fn current_chain_id() -> u32 {
    CHAIN_ID_REGTEST
}

pub fn median_time_past(blocks: &[crate::Block], current_height: u64) -> u64 {
    if current_height == 0 {
        return 0;
    }
    let start = current_height.saturating_sub(MEDIAN_TIME_WINDOW as u64);
    let mut times: Vec<u64> = blocks[start as usize..].iter().map(|b| b.timestamp).collect();
    times.sort();
    times[times.len() / 2]
}

pub fn validate_timestamp(
    block: &crate::Block,
    prev_blocks: &[crate::Block],
    now: u64,
) -> Result<(), crate::error::StrangecoinError> {
    // Genesis block (index 0) has timestamp 0 - allow it
    if block.index == 0 {
        return Ok(());
    }
    let mtp = median_time_past(prev_blocks, block.index);
    if block.timestamp <= mtp {
        return Err(crate::error::StrangecoinError::TimestampTooOld);
    }
    if block.timestamp > now + MAX_FUTURE_TIME {
        return Err(crate::error::StrangecoinError::TimestampInFuture);
    }
    Ok(())
}

pub fn recover_pubkey_from_sig(signature: &[u8], message: &[u8]) -> Result<secp256k1::PublicKey, crate::error::StrangecoinError> {
    if signature.len() != 65 {
        return Err(crate::error::StrangecoinError::InvalidSignature);
    }
    let secp = secp256k1::Secp256k1::new();
    let msg_hash = blake3::hash(message);
    let msg = secp256k1::Message::from_digest_slice(msg_hash.as_bytes())
        .map_err(|_| crate::error::StrangecoinError::InvalidSignature)?;
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(&signature[..64]);
    let rec_id = secp256k1::ecdsa::RecoveryId::from_i32(signature[64] as i32)
        .map_err(|_| crate::error::StrangecoinError::InvalidSignature)?;
    let sig = secp256k1::ecdsa::RecoverableSignature::from_compact(&sig_bytes, rec_id)
        .map_err(|_| crate::error::StrangecoinError::InvalidSignature)?;
    let recovered_pk = secp.recover_ecdsa(&msg, &sig)
        .map_err(|_| crate::error::StrangecoinError::InvalidSignature)?;
    Ok(recovered_pk)
}

pub fn verify_transaction(tx: &crate::Transaction) -> Result<(), crate::error::StrangecoinError> {
    if tx.is_coinbase {
        return Ok(());
    }
    let pk = recover_pubkey_from_sig(&tx.signature, &crate::serialize::serialize_transaction(tx))?;
    if crate::address::address_from_public_key(&pk) != tx.sender {
        return Err(crate::error::StrangecoinError::InvalidSignature);
    }
    Ok(())
}