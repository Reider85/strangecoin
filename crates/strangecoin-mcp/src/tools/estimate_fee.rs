//! `estimate_fee(tx)` — spec §2.6. Stage 4 fixed-gas-price semantics: the
//! fee market is a Stage 5 item (ADR-0032 / SCIP-0003, `fee_market.rs` is a
//! TODO), so `fee_sc` is `gas_units × FEE_SC_PER_GAS` with
//! [`FEE_SC_PER_GAS`] — today `0`, matching the chain's current zero-fee
//! default; the *gas estimate* is the useful half of the answer.

use serde_json::{json, Value};
use strangecoin_core::vm::gas::BLOCK_GAS_LIMIT;

use super::call_contract::build_calldata;
use crate::{McpBackend, McpError};

/// Fixed Stage 4 gas price in atomic units per gas unit. Zero until the
/// EIP-1559 fee market lands (Stage 5, SCIP-gated) — every constructor on
/// the current chain defaults `fee = 0` and the mempool requires no
/// minimum, so recommending a non-zero fee would fabricate economics no ADR
/// defines.
pub const FEE_SC_PER_GAS: u64 = 0;

pub fn schema() -> Value {
    json!({
        "name": "estimate_fee",
        "description": "Estimate gas and fee for a transaction. Stage 4 uses fixed gas price; Stage 5 will use EIP-1559.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "tx": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "enum": ["transfer", "call", "create"]},
                        "to": {"type": "string"},
                        "amount": {"type": "string"},
                        "method": {"type": "string"},
                        "args": {"type": "array"},
                        "calldata": {"type": "string"}
                    },
                    "required": ["type"]
                }
            },
            "required": ["tx"]
        },
        "outputSchema": {
            "type": "object",
            "properties": {
                "gas_units": {"type": "integer"},
                "fee_sc": {"type": "string"}
            }
        }
    })
}

pub fn call(backend: &dyn McpBackend, args: &Value) -> Result<Value, McpError> {
    let tx = args
        .get("tx")
        .ok_or_else(|| McpError::InvalidParams("missing `tx` object".into()))?;
    let kind = tx
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::InvalidParams("missing `tx.type`".into()))?;

    let gas_units = match kind {
        // Transfer-only frames commit with gas_used = 0 (apply_call EOA
        // path) — the estimate mirrors on-chain semantics.
        "transfer" => 0,
        "call" => {
            let to = tx
                .get("to")
                .and_then(Value::as_str)
                .ok_or_else(|| McpError::InvalidParams("`tx.to` required for type=call".into()))?;
            // Accept both raw `tx.to` and address-shaped validation.
            strangecoin_core::address::decode_address(to)
                .map_err(|err| McpError::InvalidParams(format!("invalid `tx.to`: {err}")))?;
            let calldata = build_calldata(tx)?;
            let outcome = backend.call_contract(to, calldata, BLOCK_GAS_LIMIT)?;
            outcome.gas_used
        }
        // The spec schema carries no deploy code for `create`, so a dry-run
        // is impossible: report the block gas budget as an upper bound.
        "create" => BLOCK_GAS_LIMIT,
        other => {
            return Err(McpError::InvalidParams(format!(
                "unknown `tx.type` `{other}` (expected transfer|call|create)"
            )))
        }
    };

    Ok(json!({
        "gas_units": gas_units,
        "fee_sc": (gas_units.saturating_mul(FEE_SC_PER_GAS)).to_string(),
    }))
}
