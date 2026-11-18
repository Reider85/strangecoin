//! The 4 MCP resources (spec `docs/spec/mcp_server.md` §3): `chain://`
//! URI handlers for `resources/list`, `resources/templates/list` and
//! `resources/read`.

pub mod block;
pub mod contract;
pub mod tip;
pub mod tx;

use serde_json::Value;

use crate::{McpBackend, McpError};

/// Resource entries for `resources/list` — the concrete `chain://` URIs in
/// fixed order (deterministic, no map iteration).
pub fn list() -> Vec<Value> {
    vec![tip::entry(), block::entry(), tx::entry(), contract::entry()]
}

/// The same four as URI templates (`resources/templates/list` — MCP's
/// template primitive; our spec §7.4 lists them through `resources/list`,
/// both surfaces are served for client compatibility).
pub fn templates() -> Vec<Value> {
    list()
}

/// `resources/read` dispatcher: `chain://` scheme, `/`-separated segments.
pub fn read(backend: &dyn McpBackend, uri: &str) -> Result<(String, Value), McpError> {
    let rest = uri.strip_prefix("chain://").ok_or_else(|| {
        McpError::InvalidParams(format!(
            "unsupported resource scheme in `{uri}` (expected chain://)"
        ))
    })?;

    if rest == "tip" {
        return tip::read(backend);
    }

    let (kind, id) = rest
        .split_once('/')
        .ok_or_else(|| McpError::InvalidParams(format!("unknown resource `{uri}`")))?;
    match kind {
        "block" => block::read(backend, id),
        "tx" => tx::read(backend, id),
        "contract" => contract::read(backend, id),
        other => Err(McpError::InvalidParams(format!(
            "unknown resource kind `{other}` (expected tip|block|tx|contract)"
        ))),
    }
}

pub(crate) const MIME: &str = "application/json";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_is_four_fixed_entries() {
        let entries = list();
        assert_eq!(entries.len(), 4);
        let uris: Vec<&str> = entries.iter().map(|e| e["uri"].as_str().unwrap()).collect();
        assert_eq!(
            uris,
            vec![
                "chain://tip",
                "chain://block/{height_or_hash}",
                "chain://tx/{txid}",
                "chain://contract/{address}",
            ]
        );
    }

    #[test]
    fn read_rejects_foreign_schemes() {
        // Backend never touched: scheme validation happens first.
        struct Unreachable;
        impl McpBackend for Unreachable {
            fn tip_header(&self) -> Option<strangecoin_core::types::BlockHeader> {
                unreachable!()
            }
            fn block_by_height(&self, _: u64) -> Option<strangecoin_core::types::Block> {
                unreachable!()
            }
            fn block_by_hash(&self, _: &str) -> Option<strangecoin_core::types::Block> {
                unreachable!()
            }
            fn get_balance(&self, _: &str) -> u64 {
                unreachable!()
            }
            fn get_nonce(&self, _: &str) -> u64 {
                unreachable!()
            }
            fn pending_balance(&self, _: &str) -> u64 {
                unreachable!()
            }
            fn submit_tx(
                &self,
                _: strangecoin_core::types::Transaction,
            ) -> Result<[u8; 32], McpError> {
                unreachable!()
            }
            fn call_contract(
                &self,
                _: &str,
                _: Vec<u8>,
                _: u64,
            ) -> Result<crate::CallOutcome, McpError> {
                unreachable!()
            }
            fn get_tx_with_receipt(&self, _: &str) -> Result<crate::TxRecord, McpError> {
                unreachable!()
            }
            fn contract_info(&self, _: &str) -> Result<crate::ContractInfo, McpError> {
                unreachable!()
            }
            fn subscribe_events(&self) -> tokio::sync::mpsc::Receiver<crate::McpEvent> {
                unreachable!()
            }
        }
        assert!(read(&Unreachable, "http://example.com").is_err());
        assert!(read(&Unreachable, "chain://nope").is_err());
    }
}
