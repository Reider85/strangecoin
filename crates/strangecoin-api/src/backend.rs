//! Node-side data plane every `agent_*` method is a pure function of
//! (AI-P04).
//!
//! Same strangler split as `strangecoin_net::BlockchainView` and
//! `strangecoin_mcp::McpBackend`: this crate owns the JSON-RPC protocol
//! shell, the trait impl lives in the root crate against
//! [`strangecoin::BlockchainFacade`] (gate 3 of prompt-stage3.md §8.3 —
//! business logic stays out of this crate; no consensus rules touched).

use strangecoin_core::types::Transaction;

/// Backend failure — mapped onto [`crate::error::AgentError`] at the
/// method layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// Address / txid unknown to the chain.
    NotFound(String),
    /// The node rejected the operation (mempool rules, insufficient
    /// balance, VM failure in a simulation…).
    Rejected(String),
    /// The node was built without the feature the operation needs
    /// (e.g. `vm-wasm` for simulations).
    Unsupported(String),
    /// Internal backend failure.
    Internal(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::NotFound(what) => write!(f, "not found: {what}"),
            BackendError::Rejected(why) => write!(f, "rejected: {why}"),
            BackendError::Unsupported(what) => write!(f, "unsupported: {what}"),
            BackendError::Internal(why) => write!(f, "backend error: {why}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// Confirmation state of a submitted transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxConfirmation {
    /// In the mempool, not yet mined.
    Mempool,
    /// Included in a block at this height.
    Confirmed { height: u64 },
}

/// Outcome of a read-only [`AgentBackend::simulate_call`] — a dry-run
/// against a cloned state that is discarded (never committed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimOutcome {
    /// Gas the frame consumed (0 for an EOA target — the transfer-only
    /// path commits `gas_used = 0`, mirroring on-chain receipts).
    pub gas_used: u64,
    /// Whether the simulated frame succeeded.
    pub success: bool,
}

/// Pre-state injected into a [`simulate_call`](AgentBackend::simulate_call)
/// clone before the frame runs — lets the estimator simulate a *later*
/// step (e.g. the EscrowVault `release`) whose storage record and vault
/// balance only exist after the deposit it is paired with. Seeds are pure
/// data (no clock, no RNG) — the estimator stays deterministic.
#[derive(Debug, Clone, Default)]
pub struct StateSeed {
    /// `(storage key, record)` pairs written into the callee's storage map.
    pub storage: Vec<([u8; 32], Vec<u8>)>,
    /// `(address, balance)` overrides applied to the cloned account map.
    pub balances: Vec<(String, u64)>,
}

/// One chain transaction with its block context — the input to activity
/// classification ([`crate::methods`] filters + classifies; the backend
/// only fetches).
#[derive(Debug, Clone)]
pub struct TxRecord {
    /// Canonical txid (hex, no `0x` prefix).
    pub txid: String,
    /// Height of the containing block.
    pub height: u64,
    /// Block timestamp (chain time — deterministic, never wall clock).
    pub timestamp: u64,
    /// The transaction itself.
    pub tx: Transaction,
}

/// The node-side data plane every `agent_*` method is a pure function of.
///
/// Implemented for `BlockchainFacade` in the root crate behind the
/// `agent-api` feature (same strangler split as `McpBackend`).
pub trait AgentBackend: Send + Sync {
    /// Confirmed balance at the current tip, atomic units.
    fn get_balance(&self, address: &str) -> u64;
    /// Confirmed nonce (account state).
    fn get_nonce(&self, address: &str) -> u64;
    /// Next mempool-valid nonce: `account.nonce + pending + 1` (BUG-S0-025
    /// 1-based mempool convention).
    fn next_nonce(&self, address: &str) -> u64;
    /// Chain id the node validates (invariant #10) — binds addresses and
    /// signed payloads.
    fn chain_id(&self) -> u32;
    /// Current tip height.
    fn tip_height(&self) -> u64;
    /// Validate + queue a signed transaction in the mempool. The txid is
    /// computed by the caller (`serialize::txid`) so rejections can still
    /// report it.
    fn submit_tx(&self, tx: Transaction) -> Result<[u8; 32], BackendError>;
    /// Confirmation state of a txid (hex, `0x`-prefix optional): mempool
    /// vs confirmed-at-height vs unknown.
    fn tx_confirmation(&self, txid_hex: &str) -> Result<TxConfirmation, BackendError>;
    /// Dry-run one call frame against a **cloned** state (discarded after
    /// the run — non-read-only so contracts that write storage, e.g. the
    /// EscrowVault deposit, simulate faithfully). `value` seeds `sc.value()`.
    fn simulate_call(
        &self,
        from: &str,
        to: &str,
        data: Vec<u8>,
        value: u64,
        gas_limit: u64,
    ) -> Result<SimOutcome, BackendError>;
    /// Most-recent-first transactions whose sender or receiver is
    /// `address`, scanned over the last `max_blocks` heights (bounded —
    /// the Stage 4 indexer S4-core-P07 replaces this scan).
    fn txs_involving(&self, address: &str, max_blocks: u64) -> Vec<TxRecord>;
}

/// In-memory [`AgentBackend`] for tests and offline tooling — a scripted
/// chain: balances/nonces set directly, submitted txs recorded, activity
/// supplied as a fixture. Not for production (no validation, no VM).
#[derive(Default)]
pub struct MockBackend {
    /// address -> (balance, nonce).
    pub accounts: std::collections::HashMap<String, (u64, u64)>,
    /// Submitted txids in submit order.
    pub submitted: Vec<[u8; 32]>,
    /// Fixture activity, returned verbatim by [`txs_involving`](AgentBackend::txs_involving).
    pub activity: Vec<TxRecord>,
    /// Chain id reported by [`chain_id`](AgentBackend::chain_id).
    pub chain_id: u32,
    /// Tip height reported by [`tip_height`](AgentBackend::tip_height).
    pub tip: u64,
    /// Gas reported by [`simulate_call`](AgentBackend::simulate_call).
    pub simulated_gas: u64,
}

impl MockBackend {
    /// Empty mock on the given chain id.
    pub fn new(chain_id: u32) -> Self {
        MockBackend {
            chain_id,
            ..Default::default()
        }
    }

    /// Credit `balance` and set `nonce` for `address`.
    pub fn set_account(&mut self, address: &str, balance: u64, nonce: u64) {
        self.accounts.insert(address.to_string(), (balance, nonce));
    }
}

impl AgentBackend for MockBackend {
    fn get_balance(&self, address: &str) -> u64 {
        self.accounts.get(address).map(|(b, _)| *b).unwrap_or(0)
    }

    fn get_nonce(&self, address: &str) -> u64 {
        self.accounts.get(address).map(|(_, n)| *n).unwrap_or(0)
    }

    fn next_nonce(&self, address: &str) -> u64 {
        self.get_nonce(address) + 1
    }

    fn chain_id(&self) -> u32 {
        self.chain_id
    }

    fn tip_height(&self) -> u64 {
        self.tip
    }

    fn submit_tx(&self, _tx: Transaction) -> Result<[u8; 32], BackendError> {
        // Recorded by the caller in tests via `submitted` when needed;
        // the mock is immutable through `&self` (interior mutability would
        // hide test assertions — tests push into `submitted` directly).
        Ok([0u8; 32])
    }

    fn tx_confirmation(&self, txid_hex: &str) -> Result<TxConfirmation, BackendError> {
        let bytes = hex::decode(txid_hex.strip_prefix("0x").unwrap_or(txid_hex))
            .map_err(|e| BackendError::NotFound(format!("bad txid hex: {e}")))?;
        let txid: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| BackendError::NotFound("txid must be 32 bytes".into()))?;
        if self.submitted.contains(&txid) {
            Ok(TxConfirmation::Mempool)
        } else {
            Err(BackendError::NotFound(format!("txid {txid_hex}")))
        }
    }

    fn simulate_call(
        &self,
        _from: &str,
        _to: &str,
        _data: Vec<u8>,
        _value: u64,
        _gas_limit: u64,
    ) -> Result<SimOutcome, BackendError> {
        Ok(SimOutcome {
            gas_used: self.simulated_gas,
            success: true,
        })
    }

    fn txs_involving(&self, _address: &str, _max_blocks: u64) -> Vec<TxRecord> {
        self.activity.clone()
    }
}
