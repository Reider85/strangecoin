//! `chain://contract/{address}` — code + storage root (spec §3.4).

use serde_json::{json, Value};
use strangecoin_core::address::decode_address;

use super::MIME;
use crate::{McpBackend, McpError};

pub fn entry() -> Value {
    json!({
        "uri": "chain://contract/{address}",
        "name": "contract",
        "description": "Contract account: WASM code (base64), storage root, ABI (null — the node does not persist ABIs).",
        "mimeType": MIME,
    })
}

pub fn read(backend: &dyn McpBackend, address: &str) -> Result<(String, Value), McpError> {
    decode_address(address)
        .map_err(|err| McpError::InvalidParams(format!("invalid contract address: {err}")))?;
    let info = backend.contract_info(address)?;
    Ok((
        MIME.to_string(),
        json!({
            "address": address,
            "code": info.code.as_ref().map(|bytes| {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD.encode(bytes)
            }),
            "storage_root": info.storage_root.map(hex::encode),
            "abi": info.abi_json,
        }),
    ))
}
