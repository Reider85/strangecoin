//! `call_contract(address, method, args)` — spec §2.5, read-only VM
//! execution (eth_call equivalent).

use serde_json::{json, Value};
use strangecoin_core::vm::gas::BLOCK_GAS_LIMIT;

use super::{encode_arg, req_str, require_address};
use crate::{McpBackend, McpError};

pub fn schema() -> Value {
    json!({
        "name": "call_contract",
        "description": "Read-only contract call (eth_call equivalent). Does not modify state.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "address": {"type": "string", "pattern": "^(sc1|tsc1|rsc1)[a-z0-9]{38,}$"},
                "method": {"type": "string", "description": "4-byte blake3 method selector (hex) or ABI method name"},
                "args": {"type": "array"},
                "calldata": {"type": "string", "description": "raw hex calldata; overrides method/args when present"}
            },
            "required": ["address", "method", "args"]
        },
        "outputSchema": {
            "type": "object",
            "properties": {
                "result": {"type": "string", "description": "hex-encoded return bytes"},
                "gas_used": {"type": "integer"},
                "logs": {"type": "array", "items": {"type": "object"}},
                "status": {"type": "string"}
            }
        }
    })
}

/// Build call payload from `calldata` (raw override) or `method` + `args`
/// (spec §2.5 argument rule — documented in the spec amendment: `0x`-prefixed
/// strings are hex bytes, other strings UTF-8, integers 8-byte LE). An ABI
/// *name* cannot be encoded because the node persists no ABI (pre-existing
/// gap); the 4-byte selector form is the supported path.
pub fn build_calldata(args: &Value) -> Result<Vec<u8>, McpError> {
    if let Some(raw) = super::opt_str(args, "calldata") {
        let bytes = super::decode_hex("calldata", raw)?;
        if bytes.is_empty() {
            return Err(McpError::InvalidParams("`calldata` is empty".into()));
        }
        return Ok(bytes);
    }

    let method = req_str(args, "method")?;
    let method_clean = method.strip_prefix("0x").unwrap_or(method);
    if method_clean.len() != 8 || !method_clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(McpError::InvalidParams(
            "`method` must be a 4-byte hex selector (8 hex chars); ABI method names cannot \
             be encoded because the node does not persist contract ABIs — pass raw hex via \
             `calldata` instead"
                .into(),
        ));
    }
    let mut out = hex::decode(method_clean)
        .map_err(|err| McpError::InvalidParams(format!("bad selector hex: {err}")))?;
    if let Some(items) = args.get("args").and_then(Value::as_array) {
        for item in items {
            encode_arg(item, &mut out)?;
        }
    }
    Ok(out)
}

pub fn call(backend: &dyn McpBackend, args: &Value) -> Result<Value, McpError> {
    let address = require_address(args, "address")?;
    let calldata = build_calldata(args)?;

    let outcome = backend.call_contract(&address, calldata, BLOCK_GAS_LIMIT)?;
    if outcome.status != strangecoin_core::vm::TxStatus::Success {
        return Err(McpError::Rejected(format!(
            "execution finished with status {:?} (gas_used {})",
            outcome.status, outcome.gas_used
        )));
    }

    let logs: Vec<Value> = outcome
        .logs
        .iter()
        .map(|log| {
            json!({
                "address": log.address,
                "topics": log.topics.iter().map(hex::encode).collect::<Vec<_>>(),
                "data": hex::encode(&log.data),
            })
        })
        .collect();

    Ok(json!({
        "result": hex::encode(&outcome.return_data),
        "gas_used": outcome.gas_used,
        "logs": logs,
        "status": format!("{:?}", outcome.status),
    }))
}
