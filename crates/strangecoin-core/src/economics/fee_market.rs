//! Fee market (Stage 5 placeholder — ADR-0032 / SCIP-0003).
//!
//! The only live constant today is [`FEE_SC_PER_GAS`]: the fixed Stage 4
//! gas price every fee quote is computed from. It is `0` — the current
//! chain defaults `fee = 0` on every constructor and the mempool requires
//! no minimum, so quoting a non-zero price would fabricate economics no
//! ADR defines (the MCP `estimate_fee` tool documents the same reasoning).
//! The EIP-1559 base-fee surface lands with the Stage 5 fee market and
//! will replace this constant through SCIP (consensus-adjacent pricing).

/// Fixed Stage 4 gas price in atomic units per gas unit.
///
/// Single source of truth shared by the MCP `estimate_fee` tool and the
/// agent JSON-RPC `agent_estimateTaskCost` method (AI-P04) — both must
/// quote the same price. Zero until the Stage 5 fee market lands.
pub const FEE_SC_PER_GAS: u64 = 0;
