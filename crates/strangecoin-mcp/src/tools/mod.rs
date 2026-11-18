//! The 7 MCP tools (spec `docs/spec/mcp_server.md` §2): schemas for
//! `tools/list` and pure dispatch for `tools/call`.

pub mod call_contract;
pub mod estimate_fee;
pub mod get_balance;
pub mod get_block;
pub mod get_nonce;
pub mod submit_tx;
pub mod subscribe_events;

use serde_json::Value;

use crate::{McpBackend, McpError, McpServer, RequestCtx};

/// Tool names in spec order (`tools/list` output is deterministic — fixed
/// declaration order, no HashMap iteration anywhere on this path).
pub fn schemas() -> Vec<Value> {
    vec![
        get_block::schema(),
        get_balance::schema(),
        get_nonce::schema(),
        submit_tx::schema(),
        call_contract::schema(),
        estimate_fee::schema(),
        subscribe_events::schema(),
    ]
}

pub fn names() -> [&'static str; 7] {
    [
        "get_block",
        "get_balance",
        "get_nonce",
        "submit_tx",
        "call_contract",
        "estimate_fee",
        "subscribe_events",
    ]
}

/// Dispatch one `tools/call`.
pub fn call(
    server: &McpServer,
    ctx: &RequestCtx,
    name: &str,
    args: &Value,
) -> Result<Value, McpError> {
    let backend_arc = server.backend();
    let backend: &dyn McpBackend = backend_arc.as_ref();
    match name {
        "get_block" => get_block::call(backend, args),
        "get_balance" => get_balance::call(backend, args),
        "get_nonce" => get_nonce::call(backend, args),
        "submit_tx" => submit_tx::call(backend, args),
        "call_contract" => call_contract::call(backend, args),
        "estimate_fee" => estimate_fee::call(backend, args),
        "subscribe_events" => subscribe_events::call(backend, ctx, args),
        other => Err(McpError::NotFound(format!("unknown tool `{other}`"))),
    }
}

// ---------------------------------------------------------------- args utils

pub(crate) fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, McpError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::InvalidParams(format!("missing string argument `{key}`")))
}

pub(crate) fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Validate a bech32 address (any network HRP: `sc1`/`tsc1`/`rsc1`).
pub(crate) fn require_address(args: &Value, key: &str) -> Result<String, McpError> {
    let address = req_str(args, key)?.to_string();
    strangecoin_core::address::decode_address(&address)
        .map_err(|err| McpError::InvalidParams(format!("invalid `{key}`: {err}")))?;
    Ok(address)
}

/// Hex decode with optional `0x` prefix (protocol fields are lowercase hex
/// per the spec schemas).
pub(crate) fn decode_hex(field: &str, value: &str) -> Result<Vec<u8>, McpError> {
    let cleaned = value.strip_prefix("0x").unwrap_or(value);
    hex::decode(cleaned)
        .map_err(|err| McpError::InvalidParams(format!("`{field}` is not valid hex: {err}")))
}

pub(crate) fn decode_hex32(field: &str, value: &str) -> Result<[u8; 32], McpError> {
    let bytes = decode_hex(field, value)?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| McpError::InvalidParams(format!("`{field}` must be 32 bytes (64 hex chars)")))
}

/// Argument rule for `call_contract`/`estimate_fee` payloads (spec §2.5,
/// documented in the spec amendment): strings are hex bytes when they carry
/// a `0x` prefix (even-length hex), otherwise UTF-8; integers are 8-byte
/// little-endian; nested arrays flatten; objects are rejected (no ABI on the
/// node to type them).
pub(crate) fn encode_arg(value: &Value, out: &mut Vec<u8>) -> Result<(), McpError> {
    match value {
        Value::String(s) => {
            if let Some(hex_body) = s.strip_prefix("0x") {
                if hex_body.len() % 2 != 0 {
                    return Err(McpError::InvalidParams(
                        "0x-prefixed argument must have even hex length".into(),
                    ));
                }
                let bytes = hex::decode(hex_body).map_err(|err| {
                    McpError::InvalidParams(format!("bad 0x argument hex: {err}"))
                })?;
                out.extend_from_slice(&bytes);
            } else {
                out.extend_from_slice(s.as_bytes());
            }
            Ok(())
        }
        Value::Number(n) => {
            let v = n.as_u64().ok_or_else(|| {
                McpError::InvalidParams("numeric arguments must be unsigned integers".into())
            })?;
            out.extend_from_slice(&v.to_le_bytes());
            Ok(())
        }
        Value::Bool(b) => {
            out.push(u8::from(*b));
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                encode_arg(item, out)?;
            }
            Ok(())
        }
        Value::Null => Ok(()),
        Value::Object(_) => Err(McpError::InvalidParams(
            "object arguments are unsupported: the node persists no ABI (pass hex calldata \
             via the `calldata` field instead)"
                .into(),
        )),
    }
}
