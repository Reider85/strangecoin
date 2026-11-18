//! `chain://block/{height_or_hash}` — full block (spec §3.2).

use serde_json::Value;

use super::MIME;
use crate::{block_json, McpBackend, McpError};

pub fn entry() -> Value {
    serde_json::json!({
        "uri": "chain://block/{height_or_hash}",
        "name": "block",
        "description": "Full block (header + transactions) by decimal height or 64-char hex hash.",
        "mimeType": MIME,
    })
}

pub fn read(backend: &dyn McpBackend, id: &str) -> Result<(String, Value), McpError> {
    let block = if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() {
        let height: u64 = id
            .parse()
            .map_err(|_| McpError::InvalidParams(format!("block height `{id}` out of range")))?;
        backend
            .block_by_height(height)
            .ok_or_else(|| McpError::NotFound(format!("block at height {height} not found")))?
    } else {
        crate::tools::decode_hex32("block id", id)?;
        backend
            .block_by_hash(id)
            .ok_or_else(|| McpError::NotFound(format!("block `{id}` not found")))?
    };
    Ok((MIME.to_string(), block_json(&block)))
}
