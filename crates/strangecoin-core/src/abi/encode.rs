//! Canonical ABI argument encoding (S1.5-P12): head/tail split, 32-byte
//! static words, 4-byte big-endian offsets/lengths. See `abi/mod.rs` for
//! the format contract; golden vectors in `tests/abi_roundtrip.rs` pin it.

use super::{AbiError, AbiType, AbiValue};

/// Encode `values` against the declared `params` into canonical calldata.
pub fn encode_args(params: &[super::AbiParam], values: &[AbiValue]) -> Result<Vec<u8>, AbiError> {
    if params.len() != values.len() {
        return Err(AbiError::ArityMismatch {
            expected: params.len(),
            got: values.len(),
        });
    }
    let types: Vec<&AbiType> = params.iter().map(|p| &p.param_type).collect();
    encode_tuple(&types, values)
}

pub(crate) fn encode_tuple(types: &[&AbiType], values: &[AbiValue]) -> Result<Vec<u8>, AbiError> {
    if types.len() != values.len() {
        return Err(AbiError::ArityMismatch {
            expected: types.len(),
            got: values.len(),
        });
    }
    let mut head_len = 0usize;
    for t in types {
        head_len += if t.is_dynamic() {
            4
        } else {
            t.static_size().unwrap_or(0)
        };
    }
    let mut head = Vec::with_capacity(head_len);
    let mut tail = Vec::new();
    for (t, v) in types.iter().zip(values.iter()) {
        if t.is_dynamic() {
            let offset =
                u32::try_from(head_len + tail.len()).map_err(|_| AbiError::InvalidOffset {
                    offset: u32::MAX,
                    bound: head_len + tail.len(),
                })?;
            head.extend_from_slice(&offset.to_be_bytes());
            encode_dynamic_body(t, v, &mut tail)?;
        } else {
            head.extend(encode_static(t, v)?);
        }
    }
    head.extend_from_slice(&tail);
    Ok(head)
}

/// Validate the declared integer width and the canonical range of `value`.
pub(crate) fn check_uint_range(value: &[u8; 32], bits: u16) -> Result<(), AbiError> {
    AbiType::validate_bits(bits)?;
    let full = (bits / 8) as usize;
    for b in &value[..32 - full] {
        if *b != 0 {
            return Err(AbiError::InvalidValue {
                reason: format!("uint{bits} value has non-zero high bytes"),
            });
        }
    }
    Ok(())
}

/// Validate canonical sign-extension of a 32-byte two's-complement value
/// against the declared `intN` width.
pub(crate) fn check_int_range(value: &[u8; 32], bits: u16) -> Result<(), AbiError> {
    AbiType::validate_bits(bits)?;
    let keep = (bits / 8) as usize;
    let sign_byte = value[32 - keep];
    let fill = if sign_byte & 0x80 != 0 { 0xff } else { 0x00 };
    for b in &value[..32 - keep] {
        if *b != fill {
            return Err(AbiError::InvalidValue {
                reason: format!("int{bits} value is not sign-extended"),
            });
        }
    }
    Ok(())
}

fn encode_static(t: &AbiType, v: &AbiValue) -> Result<Vec<u8>, AbiError> {
    match (t, v) {
        (AbiType::Uint(bits), AbiValue::Uint(value)) => {
            check_uint_range(value, *bits)?;
            Ok(value.to_vec())
        }
        (AbiType::Int(bits), AbiValue::Int(value)) => {
            check_int_range(value, *bits)?;
            Ok(value.to_vec())
        }
        (AbiType::Bool, AbiValue::Bool(b)) => {
            let mut out = vec![0u8; 32];
            if *b {
                out[31] = 1;
            }
            Ok(out)
        }
        (AbiType::Address, AbiValue::Address(a)) => Ok(a.to_vec()),
        (AbiType::FixedArray(elem, len), AbiValue::FixedArray(vals)) => {
            if vals.len() != *len {
                return Err(AbiError::InvalidValue {
                    reason: format!("fixed array length {len}, got {}", vals.len()),
                });
            }
            let elem_refs: Vec<&AbiType> = vec![elem.as_ref(); *len];
            encode_tuple(&elem_refs, vals)
        }
        (AbiType::Tuple(elems), AbiValue::Tuple(vals)) => {
            let refs: Vec<&AbiType> = elems.iter().collect();
            encode_tuple(&refs, vals)
        }
        _ => Err(AbiError::TypeMismatch {
            expected: t.canonical_name(),
            got: value_kind(v).to_string(),
        }),
    }
}

fn encode_dynamic_body(t: &AbiType, v: &AbiValue, out: &mut Vec<u8>) -> Result<(), AbiError> {
    match (t, v) {
        (AbiType::Bytes, AbiValue::Bytes(b)) => {
            let len = u32::try_from(b.len()).map_err(|_| AbiError::InvalidValue {
                reason: "bytes longer than u32::MAX".to_string(),
            })?;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(b);
            Ok(())
        }
        (AbiType::String, AbiValue::String(s)) => {
            let bytes = s.as_bytes();
            let len = u32::try_from(bytes.len()).map_err(|_| AbiError::InvalidValue {
                reason: "string longer than u32::MAX".to_string(),
            })?;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(bytes);
            Ok(())
        }
        (AbiType::Array(elem), AbiValue::Array(vals)) => {
            let count = u32::try_from(vals.len()).map_err(|_| AbiError::InvalidValue {
                reason: "array longer than u32::MAX elements".to_string(),
            })?;
            out.extend_from_slice(&count.to_be_bytes());
            let refs: Vec<&AbiType> = vec![elem.as_ref(); vals.len()];
            out.extend(encode_tuple(&refs, vals)?);
            Ok(())
        }
        (AbiType::FixedArray(elem, len), AbiValue::FixedArray(vals)) => {
            if vals.len() != *len {
                return Err(AbiError::InvalidValue {
                    reason: format!("fixed array length {len}, got {}", vals.len()),
                });
            }
            let refs: Vec<&AbiType> = vec![elem.as_ref(); *len];
            out.extend(encode_tuple(&refs, vals)?);
            Ok(())
        }
        (AbiType::Tuple(elems), AbiValue::Tuple(vals)) => {
            let refs: Vec<&AbiType> = elems.iter().collect();
            out.extend(encode_tuple(&refs, vals)?);
            Ok(())
        }
        _ => Err(AbiError::TypeMismatch {
            expected: t.canonical_name(),
            got: value_kind(v).to_string(),
        }),
    }
}

pub(crate) fn value_kind(v: &AbiValue) -> &'static str {
    match v {
        AbiValue::Uint(_) => "uint",
        AbiValue::Int(_) => "int",
        AbiValue::Bool(_) => "bool",
        AbiValue::Address(_) => "address",
        AbiValue::Bytes(_) => "bytes",
        AbiValue::String(_) => "string",
        AbiValue::Array(_) => "array",
        AbiValue::FixedArray(_) => "fixed_array",
        AbiValue::Tuple(_) => "tuple",
    }
}
