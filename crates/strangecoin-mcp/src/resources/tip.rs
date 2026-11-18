//! `chain://tip` — latest block header (spec §3.1).

use serde_json::Value;

use super::MIME;
use crate::{header_json, McpBackend, McpError};

pub fn entry() -> Value {
    serde_json::json!({
        "uri": "chain://tip",
        "name": "chain tip",
        "description": "Latest block header of the canonical chain.",
        "mimeType": MIME,
    })
}

pub fn read(backend: &dyn McpBackend) -> Result<(String, Value), McpError> {
    let header = backend
        .tip_header()
        .ok_or_else(|| McpError::NotFound("chain has no tip".into()))?;
    Ok((MIME.to_string(), header_json(&header)))
}
