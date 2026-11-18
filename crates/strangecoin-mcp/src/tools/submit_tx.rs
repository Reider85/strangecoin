//! `submit_tx(signed_tx_bytes)` — spec §2.4. Decoding happens here (pure
//! core codec) so the txid is computable even when the mempool rejects the
//! transaction; rejection is a valid `{status: "rejected"}` result, not an
//! MCP error.

use serde_json::{json, Value};

use strangecoin_core::serialize::{deserialize_transaction_signed, txid};

use super::decode_hex;
use crate::{McpBackend, McpError};

pub fn schema() -> Value {
    json!({
        "name": "submit_tx",
        "description": "Submit a signed transaction to the mempool.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "signed_tx_bytes": {"type": "string", "description": "hex-encoded canonical signed tx bytes"}
            },
            "required": ["signed_tx_bytes"]
        },
        "outputSchema": {
            "type": "object",
            "properties": {
                "txid": {"type": "string"},
                "status": {"type": "string", "enum": ["queued", "rejected"]},
                "reason": {"type": "string", "description": "present if status=rejected"}
            }
        }
    })
}

pub fn call(backend: &dyn McpBackend, args: &Value) -> Result<Value, McpError> {
    let raw_hex = super::req_str(args, "signed_tx_bytes")?;
    let bytes = decode_hex("signed_tx_bytes", raw_hex)?;
    let tx = deserialize_transaction_signed(&bytes)
        .map_err(|err| McpError::InvalidParams(format!("cannot decode signed tx: {err}")))?;
    let id = txid(&tx);

    match backend.submit_tx(tx) {
        Ok(queued_id) => Ok(json!({ "txid": hex::encode(queued_id), "status": "queued" })),
        Err(err) => Ok(json!({
            "txid": hex::encode(id),
            "status": "rejected",
            "reason": err.to_string(),
        })),
    }
}
