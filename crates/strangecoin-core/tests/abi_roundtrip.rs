//! S1.5-P12: JSON ABI round-trip (ADR-0013).
//!
//! Pins the wire format with golden vectors:
//! - byte-level `encode_args`/`decode_args` round-trip for every `AbiType`,
//! - JSON serde round-trip for `AbiDocument`,
//! - blake3 method-selector stability,
//! - `format_version: "wit-v2"` → `NotImplemented`,
//! - canonical-offset enforcement and range checks on decode.

use strangecoin_core::abi::{
    decode_args, encode_args, method_selector, method_signature, AbiDocument, AbiFunction,
    AbiParam, AbiType, AbiValue, StateMutability,
};

fn param(name: &str, ty: AbiType) -> AbiParam {
    AbiParam::new(name, ty)
}

fn roundtrip(params: &[AbiParam], values: &[AbiValue]) {
    let encoded = encode_args(params, values).expect("encode");
    let decoded = decode_args(params, &encoded).expect("decode");
    assert_eq!(&decoded, values, "round-trip mismatch for {:?}", params);
}

#[test]
fn roundtrip_every_scalar_type() {
    let addr = [0x11u8; 32];
    let uint_max = [0xffu8; 32];
    let cases: Vec<(Vec<AbiParam>, Vec<AbiValue>)> = vec![
        (
            vec![param("a", AbiType::Uint(8))],
            vec![AbiValue::uint_u64(42)],
        ),
        (
            vec![param("a", AbiType::Uint(256))],
            vec![AbiValue::Uint(uint_max)],
        ),
        (
            vec![param("a", AbiType::Int(64))],
            vec![AbiValue::int_i64(-7)],
        ),
        (
            vec![param("a", AbiType::Int(8))],
            vec![AbiValue::int_i64(-1)],
        ),
        (
            vec![param("a", AbiType::Bool)],
            vec![AbiValue::Bool(true)],
        ),
        (
            vec![param("a", AbiType::Bool)],
            vec![AbiValue::Bool(false)],
        ),
        (
            vec![param("a", AbiType::Address)],
            vec![AbiValue::Address(addr)],
        ),
        (
            vec![param("a", AbiType::Bytes)],
            vec![AbiValue::Bytes(vec![0xde, 0xad, 0xbe, 0xef])],
        ),
        (
            vec![param("a", AbiType::Bytes)],
            vec![AbiValue::Bytes(vec![])],
        ),
        (
            vec![param("a", AbiType::String)],
            vec![AbiValue::String("привет, стрейнжкоин".to_string())],
        ),
        (
            vec![param("a", AbiType::String)],
            vec![AbiValue::String(String::new())],
        ),
    ];
    for (params, values) in cases {
        roundtrip(&params, &values);
    }
}

#[test]
fn roundtrip_arrays_and_tuples() {
    let cases: Vec<(Vec<AbiParam>, Vec<AbiValue>)> = vec![
        (
            vec![param("a", AbiType::Array(Box::new(AbiType::Uint(64))))],
            vec![AbiValue::Array(vec![
                AbiValue::uint_u64(1),
                AbiValue::uint_u64(2),
                AbiValue::uint_u64(3),
            ])],
        ),
        (
            vec![param("a", AbiType::Array(Box::new(AbiType::String)))],
            vec![AbiValue::Array(vec![
                AbiValue::String("x".into()),
                AbiValue::String("yz".into()),
            ])],
        ),
        (
            vec![param(
                "a",
                AbiType::FixedArray(Box::new(AbiType::Bool), 3),
            )],
            vec![AbiValue::FixedArray(vec![
                AbiValue::Bool(true),
                AbiValue::Bool(false),
                AbiValue::Bool(true),
            ])],
        ),
        (
            vec![param(
                "a",
                AbiType::FixedArray(Box::new(AbiType::Bytes), 2),
            )],
            vec![AbiValue::FixedArray(vec![
                AbiValue::Bytes(vec![1, 2, 3]),
                AbiValue::Bytes(vec![]),
            ])],
        ),
        (
            vec![param(
                "a",
                AbiType::Tuple(vec![AbiType::Bool, AbiType::Address]),
            )],
            vec![AbiValue::Tuple(vec![
                AbiValue::Bool(true),
                AbiValue::Address([0x22; 32]),
            ])],
        ),
        (
            vec![param("a", AbiType::Tuple(vec![]))],
            vec![AbiValue::Tuple(vec![])],
        ),
        // Mixed static/dynamic tuple: head/tail split with offsets.
        (
            vec![
                param("x", AbiType::Uint(32)),
                param("s", AbiType::String),
                param("flag", AbiType::Bool),
                param("raw", AbiType::Bytes),
            ],
            vec![
                AbiValue::uint_u64(7),
                AbiValue::String("hello".into()),
                AbiValue::Bool(false),
                AbiValue::Bytes(vec![9, 8, 7]),
            ],
        ),
        // Nested: tuple containing a dynamic array of tuples.
        (
            vec![param(
                "a",
                AbiType::Tuple(vec![
                    AbiType::Uint(8),
                    AbiType::Array(Box::new(AbiType::Tuple(vec![
                        AbiType::Address,
                        AbiType::String,
                    ]))),
                ]),
            )],
            vec![AbiValue::Tuple(vec![
                AbiValue::uint_u64(1),
                AbiValue::Array(vec![
                    AbiValue::Tuple(vec![
                        AbiValue::Address([0x33; 32]),
                        AbiValue::String("a".into()),
                    ]),
                    AbiValue::Tuple(vec![
                        AbiValue::Address([0x44; 32]),
                        AbiValue::String("b".into()),
                    ]),
                ]),
            ])],
        ),
        // Empty dynamic array.
        (
            vec![param("a", AbiType::Array(Box::new(AbiType::Uint(8))))],
            vec![AbiValue::Array(vec![])],
        ),
    ];
    for (params, values) in cases {
        roundtrip(&params, &values);
    }
}

#[test]
fn transfer_signature_encodes_static_words_head_only() {
    // Golden layout: address word ‖ uint256 word — 64 bytes, no tail.
    let params = vec![
        param("to", AbiType::Address),
        param("amount", AbiType::Uint(256)),
    ];
    let mut amount = [0u8; 32];
    amount[31] = 5;
    let values = vec![AbiValue::Address([0xaa; 32]), AbiValue::Uint(amount)];
    let encoded = encode_args(&params, &values).unwrap();
    assert_eq!(encoded.len(), 64);
    assert_eq!(&encoded[..32], &[0xaa; 32]);
    assert_eq!(&encoded[32..], &amount);
}

#[test]
fn method_selector_matches_golden_vectors() {
    // Golden vectors: first 4 bytes of blake3(signature). Stable across
    // runs and platforms — any change here is a wire-format break.
    let transfer = vec![
        param("to", AbiType::Address),
        param("amount", AbiType::Uint(256)),
    ];
    assert_eq!(
        method_signature("transfer", &transfer),
        "transfer(address,uint256)"
    );
    assert_eq!(method_selector("transfer", &transfer), [0xce, 0xae, 0xeb, 0xc1]);

    let increment = vec![param("by", AbiType::Uint(64))];
    assert_eq!(
        method_signature("increment", &increment),
        "increment(uint64)"
    );
    assert_eq!(method_selector("increment", &increment), [0xd0, 0x66, 0x4d, 0xc7]);

    let no_args: Vec<AbiParam> = vec![];
    assert_eq!(method_signature("totalSupply", &no_args), "totalSupply()");
    assert_eq!(method_selector("totalSupply", &no_args), [0x1e, 0x92, 0x6a, 0x5b]);
}

#[test]
fn method_selector_differs_for_different_param_types() {
    let a = vec![param("f", AbiType::Uint(64))];
    let b = vec![param("f", AbiType::Uint(128))];
    let c = vec![param("g", AbiType::Uint(64))];
    assert_ne!(method_selector("f", &a), method_selector("f", &b));
    assert_ne!(method_selector("f", &a), method_selector("g", &c));
}

#[test]
fn json_document_roundtrip() {
    let doc = AbiDocument::new(
        "Counter",
        vec![
            AbiFunction {
                name: "increment".to_string(),
                inputs: vec![param("by", AbiType::Uint(64))],
                outputs: vec![param("", AbiType::Uint(64))],
                state_mutability: StateMutability::Nonpayable,
            },
            AbiFunction {
                name: "get".to_string(),
                inputs: vec![],
                outputs: vec![param("", AbiType::Uint(64))],
                state_mutability: StateMutability::View,
            },
        ],
    );
    let json = serde_json::to_string_pretty(&doc).unwrap();
    let parsed: AbiDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, doc);
    assert_eq!(parsed.check_format_version(), Ok(()));
    assert_eq!(
        parsed.functions[0].selector(),
        method_selector("increment", &[param("by", AbiType::Uint(64))])
    );
    assert!(parsed.function_by_name("get").is_some());
}

#[test]
fn json_roundtrip_with_events_errors_and_nested_types() {
    use strangecoin_core::abi::{AbiErrorDecl, AbiEvent, AbiEventParam};
    let mut doc = AbiDocument::new("Token", vec![]);
    doc.events.push(AbiEvent {
        name: "Incremented".to_string(),
        inputs: vec![
            AbiEventParam {
                name: "by".to_string(),
                param_type: AbiType::Uint(64),
                indexed: false,
            },
            AbiEventParam {
                name: "caller".to_string(),
                param_type: AbiType::Address,
                indexed: true,
            },
        ],
    });
    doc.errors.push(AbiErrorDecl {
        name: "Overflow".to_string(),
        inputs: vec![param("at", AbiType::Uint(64))],
    });
    doc.functions.push(AbiFunction {
        name: "batch".to_string(),
        inputs: vec![param(
            "items",
            AbiType::Array(Box::new(AbiType::Tuple(vec![
                AbiType::Address,
                AbiType::Uint(256),
            ]))),
        )],
        outputs: vec![],
        state_mutability: StateMutability::Payable,
    });
    let json = serde_json::to_string(&doc).unwrap();
    let parsed: AbiDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, doc);
    assert!(json.contains("(address,uint256)[]"));
}

#[test]
fn wit_v2_format_version_returns_not_implemented() {
    let mut doc = AbiDocument::new("C", vec![]);
    doc.format_version = "wit-v2".to_string();
    assert_eq!(
        doc.check_format_version(),
        Err(strangecoin_core::abi::AbiError::NotImplemented {
            found: "wit-v2".to_string()
        })
    );
}

#[test]
fn decode_rejects_non_canonical_offset() {
    let params = vec![
        param("x", AbiType::Bool),
        param("s", AbiType::String),
    ];
    let values = vec![AbiValue::Bool(true), AbiValue::String("hi".into())];
    let mut encoded = encode_args(&params, &values).unwrap();
    // Corrupt the offset word (bytes 32..36, right after the static bool
    // head): a non-canonical position.
    let bogus = 40u32.to_be_bytes();
    encoded[32..36].copy_from_slice(&bogus);
    assert!(matches!(
        decode_args(&params, &encoded),
        Err(strangecoin_core::abi::AbiError::NonCanonicalOffset { .. })
    ));
}

#[test]
fn decode_rejects_trailing_data() {
    let params = vec![param("x", AbiType::Bool)];
    let mut encoded = encode_args(&params, &[AbiValue::Bool(true)]).unwrap();
    encoded.push(0);
    assert!(matches!(
        decode_args(&params, &encoded),
        Err(strangecoin_core::abi::AbiError::TrailingData { .. })
    ));
}

#[test]
fn decode_rejects_truncated_input() {
    let params = vec![param("x", AbiType::Bool)];
    let encoded = encode_args(&params, &[AbiValue::Bool(true)]).unwrap();
    assert!(matches!(
        decode_args(&params, &encoded[..31]),
        Err(strangecoin_core::abi::AbiError::UnexpectedEnd(_))
    ));
}

#[test]
fn encode_rejects_out_of_range_scalars() {
    // uint8 with high bytes set.
    let params = vec![param("x", AbiType::Uint(8))];
    let mut wide = [0u8; 32];
    wide[0] = 1;
    assert!(matches!(
        encode_args(&params, &[AbiValue::Uint(wide)]),
        Err(strangecoin_core::abi::AbiError::InvalidValue { .. })
    ));

    // int8 not sign-extended (high byte 0x00 but value claims negative).
    let params = vec![param("x", AbiType::Int(8))];
    let mut bad = [0u8; 32];
    bad[31] = 0x80;
    assert!(matches!(
        encode_args(&params, &[AbiValue::Int(bad)]),
        Err(strangecoin_core::abi::AbiError::InvalidValue { .. })
    ));
}

#[test]
fn encode_rejects_arity_and_type_mismatch() {
    let params = vec![param("x", AbiType::Bool)];
    assert!(matches!(
        encode_args(&params, &[AbiValue::Bool(true), AbiValue::Bool(false)]),
        Err(strangecoin_core::abi::AbiError::ArityMismatch { .. })
    ));
    assert!(matches!(
        encode_args(&params, &[AbiValue::Bytes(vec![])]),
        Err(strangecoin_core::abi::AbiError::TypeMismatch { .. })
    ));
}

#[test]
fn decode_bool_rejects_noncanonical_word() {
    let params = vec![param("x", AbiType::Bool)];
    let mut encoded = vec![0u8; 32];
    encoded[31] = 2; // not 0 or 1
    assert!(matches!(
        decode_args(&params, &encoded),
        Err(strangecoin_core::abi::AbiError::InvalidValue { .. })
    ));
}
