//! `get_block(height | hash)` — spec §2.1.

use serde_json::{json, Value};

use super::decode_hex32;
use crate::{block_json, McpBackend, McpError};

pub fn schema() -> Value {
    json!({
        "name": "get_block",
        "description": "Get a block by height or hash.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "height": {"type": "integer", "minimum": 0},
                "hash": {"type": "string", "pattern": "^[0-9a-f]{64}$"}
            },
            "oneOf": [{"required": ["height"]}, {"required": ["hash"]}]
        },
        "outputSchema": {
            "type": "object",
            "properties": {
                "height": {"type": "integer"},
                "hash": {"type": "string"},
                "prev_hash": {"type": "string"},
                "timestamp": {"type": "integer"},
                "tx_root": {"type": "string"},
                "state_root": {"type": "string"},
                "receipts_root": {"type": "string"},
                "txs": {"type": "array", "items": {"type": "object"}}
            }
        }
    })
}

pub fn call(backend: &dyn McpBackend, args: &Value) -> Result<Value, McpError> {
    let has_height = args.get("height").is_some_and(Value::is_number);
    let has_hash = args.get("hash").is_some_and(Value::is_string);
    if has_height == has_hash {
        return Err(McpError::InvalidParams(
            "exactly one of `height` or `hash` is required".into(),
        ));
    }

    let block = if has_height {
        let height = args["height"].as_u64().ok_or_else(|| {
            McpError::InvalidParams("`height` must be an unsigned integer".into())
        })?;
        backend
            .block_by_height(height)
            .ok_or_else(|| McpError::NotFound(format!("block at height {height} not found")))?
    } else {
        let hash = args["hash"].as_str().expect("checked above");
        let _ = decode_hex32("hash", hash)?;
        backend
            .block_by_hash(hash)
            .ok_or_else(|| McpError::NotFound(format!("block `{hash}` not found")))?
    };

    Ok(block_json(&block))
}
