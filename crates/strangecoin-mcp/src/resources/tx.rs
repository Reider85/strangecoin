//! `chain://tx/{txid}` — transaction + receipt when derivable (spec §3.3).

use serde_json::{json, Value};

use super::MIME;
use crate::{McpBackend, McpError};

pub fn entry() -> Value {
    json!({
        "uri": "chain://tx/{txid}",
        "name": "transaction",
        "description": "Confirmed transaction with its receipt (receipt null when the tx is not in the current tip block).",
        "mimeType": MIME,
    })
}

pub fn read(backend: &dyn McpBackend, txid_hex: &str) -> Result<(String, Value), McpError> {
    crate::tools::decode_hex32("txid", txid_hex)?;
    let record = backend.get_tx_with_receipt(txid_hex)?;
    let tx = serde_json::to_value(&record.tx).map_err(|err| McpError::Internal(err.to_string()))?;
    let receipt = record
        .receipt
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|err| McpError::Internal(err.to_string()))?;
    Ok((
        MIME.to_string(),
        json!({
            "txid": txid_hex.to_lowercase(),
            "height": record.height,
            "tx": tx,
            "receipt": receipt,
        }),
    ))
}
