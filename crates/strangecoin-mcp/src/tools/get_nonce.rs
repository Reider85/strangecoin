//! `get_nonce(address)` — spec §2.3.

use serde_json::{json, Value};

use super::require_address;
use crate::{McpBackend, McpError};

pub fn schema() -> Value {
    json!({
        "name": "get_nonce",
        "description": "Get the current nonce (tx count) for an address.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "address": {"type": "string", "pattern": "^(sc1|tsc1|rsc1)[a-z0-9]{38,}$"}
            },
            "required": ["address"]
        },
        "outputSchema": {
            "type": "object",
            "properties": {
                "nonce": {"type": "integer"}
            }
        }
    })
}

pub fn call(backend: &dyn McpBackend, args: &Value) -> Result<Value, McpError> {
    let address = require_address(args, "address")?;
    Ok(json!({ "nonce": backend.get_nonce(&address) }))
}
