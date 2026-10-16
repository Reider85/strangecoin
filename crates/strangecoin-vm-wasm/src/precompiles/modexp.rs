//! `0x07 modexp` — modular exponentiation `base^exp mod modulus`
//! (ADR-0015, Ethereum-style ABI).
//!
//! Input: `base_len (32 BE) ‖ exp_len (32 BE) ‖ mod_len (32 BE) ‖ base ‖ exp ‖ mod`.
//! Output: `base^exp mod mod`, exactly `mod_len` bytes big-endian (left-padded
//! or truncated); `mod_len == 0` → empty output.
//! Gas: 50 + 50 per byte of result.
//!
//! Length headers beyond 8 significant bytes (or values that overflow
//! `usize` when summed with the 96-byte header) are
//! [`PrecompileError::InvalidInput`] — checked before any allocation so a
//! hostile header cannot OOM the node (threat V-21).
//!
//! Exponentiation is square-and-multiply over `u64` limbs: deterministic,
//! dependency-free, no panics. The gas schedule (linear in result bytes)
//! bounds the work well before performance matters.

use super::{PrecompileError, PrecompileOutput, modexp_gas};

/// Length of the three big-endian size headers.
pub const HEADER_LEN: usize = 96;

pub fn run(data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
    if data.len() < HEADER_LEN {
        return Err(PrecompileError::invalid_input(format!(
            "modexp expects at least {HEADER_LEN} header bytes, got {}",
            data.len()
        )));
    }

    let base_len = read_len(&data[0..32], "base_len")?;
    let exp_len = read_len(&data[32..64], "exp_len")?;
    let mod_len = read_len(&data[64..96], "mod_len")?;

    let body = HEADER_LEN
        .checked_add(base_len)
        .and_then(|n| n.checked_add(exp_len))
        .and_then(|n| n.checked_add(mod_len))
        .ok_or_else(|| PrecompileError::invalid_input("modexp input length overflow"))?;
    if data.len() < body {
        return Err(PrecompileError::invalid_input(format!(
            "modexp input truncated: need {body} bytes, got {}",
            data.len()
        )));
    }

    let gas_used = modexp_gas(mod_len as u64);
    let base_end = HEADER_LEN + base_len;
    let exp_end = base_end + exp_len;
    let base = &data[HEADER_LEN..base_end];
    let exp = &data[base_end..exp_end];
    let modulus = &data[exp_end..body];

    if mod_len == 0 {
        return Ok(PrecompileOutput {
            output: Vec::new(),
            gas_used,
        });
    }

    let output = modpow(base, exp, modulus);
    Ok(PrecompileOutput { output, gas_used })
}

/// Parse a 32-byte big-endian size header; bytes above the low 8 must be
/// zero and the value must fit in `usize`.
fn read_len(header: &[u8], field: &str) -> Result<usize, PrecompileError> {
    if header[..24].iter().any(|&b| b != 0) {
        return Err(PrecompileError::invalid_input(format!(
            "modexp {field} does not fit in 64 bits"
        )));
    }
    let mut be = [0u8; 8];
    be.copy_from_slice(&header[24..32]);
    usize::try_from(u64::from_be_bytes(be))
        .map_err(|_| PrecompileError::invalid_input(format!("modexp {field} overflow")))
}

/// `base^exp mod modulus`, result big-endian exactly `modulus.len()` bytes.
/// `modulus` must be non-empty (callers gate `mod_len == 0` earlier).
fn modpow(base: &[u8], exp: &[u8], modulus: &[u8]) -> Vec<u8> {
    let m = trim_limbs(to_le_limbs(modulus));
    if m.is_empty() || (m.len() == 1 && m[0] == 1) {
        // x mod 1 == 0 for every x.
        return vec![0u8; modulus.len()];
    }
    let b = mod_limbs(&to_le_limbs(base), &m);

    let mut result: Vec<u64> = vec![1];
    // Walk every bit of `exp`, MSB first (leading zero bytes only square
    // toward the same fixed point — deterministic, no special casing).
    let total_bits = exp.len() * 8;
    for i in (0..total_bits).rev() {
        result = mod_limbs(&mul_limbs(&result, &result), &m);
        // `i` counts from the LSB of the big-endian number: byte
        // `exp[len-1-i/8]`, bit `i%8`.
        let bit = (exp[exp.len() - 1 - i / 8] >> (i % 8)) & 1;
        if bit == 1 {
            result = mod_limbs(&mul_limbs(&result, &b), &m);
        }
    }
    from_le_limbs(&result, modulus.len())
}

/// Little-endian `u64` limbs from a big-endian byte slice.
fn to_le_limbs(be: &[u8]) -> Vec<u64> {
    be.rchunks(8)
        .map(|chunk| {
            let mut w = 0u64;
            for &byte in chunk {
                w = (w << 8) | u64::from(byte);
            }
            w
        })
        .collect()
}

/// Big-endian fixed-width bytes from little-endian limbs (pad / truncate).
fn from_le_limbs(limbs: &[u64], out_len: usize) -> Vec<u8> {
    let mut le_bytes = Vec::with_capacity(limbs.len().saturating_mul(8));
    for limb in limbs {
        le_bytes.extend_from_slice(&limb.to_le_bytes());
    }
    while le_bytes.len() > 1 && le_bytes.last() == Some(&0) {
        le_bytes.pop();
    }
    le_bytes.reverse();
    if le_bytes.len() >= out_len {
        le_bytes[le_bytes.len() - out_len..].to_vec()
    } else {
        let mut padded = vec![0u8; out_len - le_bytes.len()];
        padded.extend_from_slice(&le_bytes);
        padded
    }
}

fn trim_limbs(mut limbs: Vec<u64>) -> Vec<u64> {
    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }
    if limbs.len() == 1 && limbs[0] == 0 {
        limbs.clear();
    }
    limbs
}

/// Schoolbook multiplication on little-endian limbs.
fn mul_limbs(a: &[u64], b: &[u64]) -> Vec<u64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u64; a.len() + b.len()];
    for (i, &ai) in a.iter().enumerate() {
        if ai == 0 {
            continue;
        }
        let mut carry = 0u128;
        for (j, &bj) in b.iter().enumerate() {
            let t = u128::from(ai) * u128::from(bj) + u128::from(out[i + j]) + carry;
            out[i + j] = t as u64;
            carry = t >> 64;
        }
        out[i + b.len()] = carry as u64;
    }
    trim_limbs(out)
}

/// Compare little-endian limb slices (None only when equal length is
/// assumed by callers; here we pad conceptually by length).
fn cmp_limbs(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let len = a.len().max(b.len());
    for i in (0..len).rev() {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        match av.cmp(&bv) {
            std::cmp::Ordering::Equal => continue,
            ord => return ord,
        }
    }
    std::cmp::Ordering::Equal
}

/// `a - b` for `a >= b` on little-endian limbs.
fn sub_limbs(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut out = vec![0u64; a.len()];
    let mut borrow = 0u64;
    for i in 0..a.len() {
        let bi = b.get(i).copied().unwrap_or(0);
        let (d1, o1) = a[i].overflowing_sub(bi);
        let (d2, o2) = d1.overflowing_sub(borrow);
        out[i] = d2;
        borrow = u64::from(o1 | o2);
    }
    trim_limbs(out)
}

/// Left-shift a little-endian limb slice by one bit, appending `bit` as the
/// new least-significant bit.
fn shift_push_bit(limbs: &mut Vec<u64>, bit: u64) {
    let mut carry = bit & 1;
    for limb in limbs.iter_mut() {
        let next = *limb >> 63;
        *limb = (*limb << 1) | carry;
        carry = next;
    }
    if carry != 0 {
        limbs.push(1);
    }
}

/// Modular reduction via binary long division (shift-subtract).
fn mod_limbs(a: &[u64], m: &[u64]) -> Vec<u64> {
    debug_assert!(!m.is_empty());
    let mut rem: Vec<u64> = Vec::new();
    for i in (0..a.len() * 64).rev() {
        let bit = (a[i / 64] >> (i % 64)) & 1;
        shift_push_bit(&mut rem, bit);
        if cmp_limbs(&rem, m) != std::cmp::Ordering::Less {
            rem = sub_limbs(&rem, m);
        }
    }
    rem
}
