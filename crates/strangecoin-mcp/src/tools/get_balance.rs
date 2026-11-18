//! `get_balance(address)` — spec §2.2 (confirmed + mempool-pending).

use serde_json::{json, Value};

use super::require_address;
use crate::{McpBackend, McpError};

pub fn schema() -> Value {
    json!({
        "name": "get_balance",
        "description": "Get confirmed and pending balance for an address.",
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
                "confirmed": {"type": "string", "description": "atomic units, string to avoid precision loss"},
                "pending": {"type": "string"}
            }
        }
    })
}

pub fn call(backend: &dyn McpBackend, args: &Value) -> Result<Value, McpError> {
    let address = require_address(args, "address")?;
    Ok(json!({
        "confirmed": backend.get_balance(&address).to_string(),
        "pending": backend.pending_balance(&address).to_string(),
    }))
}
