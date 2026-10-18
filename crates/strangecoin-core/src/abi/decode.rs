//! Canonical ABI argument decoding (S1.5-P12): inverse of [`super::encode`].
//!
//! Offsets are checked against the sequential tail cursor
//! ([`AbiError::NonCanonicalOffset`]) so only the canonical layout produced
//! by `encode_args` decodes; length fields are bounds-checked before any
//! allocation (anti-OOM, invariant #7 spirit).

use super::encode::{check_int_range, check_uint_range};
use super::{AbiError, AbiType, AbiValue};

/// Decode canonical calldata into values against the declared `params`.
pub fn decode_args(params: &[super::AbiParam], data: &[u8]) -> Result<Vec<AbiValue>, AbiError> {
    let types: Vec<&AbiType> = params.iter().map(|p| &p.param_type).collect();
    let (values, consumed) = decode_tuple(&types, data)?;
    if consumed != data.len() {
        return Err(AbiError::TrailingData {
            at: consumed,
            remaining: data.len() - consumed,
        });
    }
    Ok(values)
}

/// Decode one tuple; returns the values and the number of bytes consumed.
pub(crate) fn decode_tuple(types: &[&AbiType], data: &[u8]) -> Result<(Vec<AbiValue>, usize), AbiError> {
    let mut head_len = 0usize;
    for t in types {
        head_len += if t.is_dynamic() { 4 } else { t.static_size().unwrap_or(0) };
    }
    if data.len() < head_len {
        return Err(AbiError::UnexpectedEnd(data.len()));
    }

    let mut values = Vec::with_capacity(types.len());
    let mut head_pos = 0usize;
    let mut tail_pos = head_len;
    for t in types {
        if t.is_dynamic() {
            let offset = read_u32(data, head_pos)? as usize;
            if offset != tail_pos {
                return Err(AbiError::NonCanonicalOffset {
                    expected: tail_pos as u32,
                    got: offset as u32,
                });
            }
            let (value, consumed) = decode_dynamic_body(t, &data[tail_pos..])?;
            tail_pos += consumed;
            head_pos += 4;
            values.push(value);
        } else {
            let size = t.static_size().unwrap_or(0);
            let (value, consumed) = decode_static(t, &data[head_pos..head_pos + size])?;
            head_pos += size;
            debug_assert_eq!(consumed, size);
            values.push(value);
        }
    }
    Ok((values, tail_pos))
}

fn decode_static(t: &AbiType, data: &[u8]) -> Result<(AbiValue, usize), AbiError> {
    match t {
        AbiType::Uint(bits) => {
            let raw = read_word(data)?;
            check_uint_range(&raw, *bits)?;
            Ok((AbiValue::Uint(raw), 32))
        }
        AbiType::Int(bits) => {
            let mut raw = read_word(data)?;
            check_int_range(&raw, *bits)?;
            // Sign-extend to the full 256-bit word so decode(encode(v)) == v.
            let keep = (*bits / 8) as usize;
            if keep < 32 {
                let fill = if raw[32 - keep] & 0x80 != 0 { 0xff } else { 0x00 };
                for b in &mut raw[..32 - keep] {
                    *b = fill;
                }
            }
            Ok((AbiValue::Int(raw), 32))
        }
        AbiType::Bool => {
            let word = read_word(data)?;
            if word[..31].iter().any(|b| *b != 0) || word[31] > 1 {
                return Err(AbiError::InvalidValue {
                    reason: "bool word is not 0 or 1".to_string(),
                });
            }
            Ok((AbiValue::Bool(word[31] == 1), 32))
        }
        AbiType::Address => {
            let word = read_word(data)?;
            Ok((AbiValue::Address(word), 32))
        }
        AbiType::FixedArray(elem, len) => {
            let refs: Vec<&AbiType> = vec![elem.as_ref(); *len];
            let (values, consumed) = decode_tuple(&refs, data)?;
            Ok((AbiValue::FixedArray(values), consumed))
        }
        AbiType::Tuple(elems) => {
            let refs: Vec<&AbiType> = elems.iter().collect();
            let (values, consumed) = decode_tuple(&refs, data)?;
            Ok((AbiValue::Tuple(values), consumed))
        }
        AbiType::Bytes | AbiType::String | AbiType::Array(_) => Err(AbiError::TypeMismatch {
            expected: t.canonical_name(),
            got: "static".to_string(),
        }),
    }
}

fn decode_dynamic_body(t: &AbiType, data: &[u8]) -> Result<(AbiValue, usize), AbiError> {
    match t {
        AbiType::Bytes => {
            let len = read_u32(data, 0)? as usize;
            let end = 4usize.checked_add(len).ok_or(AbiError::UnexpectedEnd(data.len()))?;
            if end > data.len() {
                return Err(AbiError::UnexpectedEnd(data.len()));
            }
            Ok((AbiValue::Bytes(data[4..end].to_vec()), end))
        }
        AbiType::String => {
            let len = read_u32(data, 0)? as usize;
            let end = 4usize.checked_add(len).ok_or(AbiError::UnexpectedEnd(data.len()))?;
            if end > data.len() {
                return Err(AbiError::UnexpectedEnd(data.len()));
            }
            let s = std::str::from_utf8(&data[4..end]).map_err(|_| AbiError::InvalidUtf8)?;
            Ok((AbiValue::String(s.to_string()), end))
        }
        AbiType::Array(elem) => {
            let count = read_u32(data, 0)? as usize;
            // Anti-OOM: every element consumes at least one byte of head or
            // tail in any non-degenerate encoding; a count larger than the
            // remaining buffer is rejected before allocation.
            if count > data.len() {
                return Err(AbiError::UnexpectedEnd(data.len()));
            }
            let refs: Vec<&AbiType> = vec![elem.as_ref(); count];
            let (values, consumed) = decode_tuple(&refs, &data[4..])?;
            Ok((AbiValue::Array(values), 4 + consumed))
        }
        AbiType::FixedArray(elem, len) => {
            let refs: Vec<&AbiType> = vec![elem.as_ref(); *len];
            let (values, consumed) = decode_tuple(&refs, data)?;
            Ok((AbiValue::FixedArray(values), consumed))
        }
        AbiType::Tuple(elems) => {
            let refs: Vec<&AbiType> = elems.iter().collect();
            let (values, consumed) = decode_tuple(&refs, data)?;
            Ok((AbiValue::Tuple(values), consumed))
        }
        AbiType::Uint(_) | AbiType::Int(_) | AbiType::Bool | AbiType::Address => {
            Err(AbiError::TypeMismatch {
                expected: t.canonical_name(),
                got: "dynamic".to_string(),
            })
        }
    }
}

fn read_word(data: &[u8]) -> Result<[u8; 32], AbiError> {
    data.get(..32)
        .and_then(|w| w.try_into().ok())
        .ok_or(AbiError::UnexpectedEnd(data.len()))
}

fn read_u32(data: &[u8], at: usize) -> Result<u32, AbiError> {
    let end = at + 4;
    let slice = data.get(at..end).ok_or(AbiError::UnexpectedEnd(data.len()))?;
    let mut b = [0u8; 4];
    b.copy_from_slice(slice);
    Ok(u32::from_be_bytes(b))
}
