//! Wallet data-source seam (UI-P03, ADR-0031 Amendment C).
//!
//! The screen layer must not depend on the node monolith (that would invert
//! the strangler direction). `WalletBackend` is implemented in the node's
//! `src/api/` module over `BlockchainFacade`; a deterministic mock is used by
//! UI unit tests, the a11y gate and the gallery.
//!
//! Method names mirror the Stage-4 JSON-RPC seam (`eth_getBalance`,
//! `eth_getTransactionByHash`, `eth_call`, `eth_sendTransaction`,
//! `agent_signAndSubmitBatch`) without requiring an HTTP client in the UI
//! crate. `estimate_fee` mirrors `agent_estimateTaskCost` and returns `0`
//! while `FEE_SC_PER_GAS = 0` holds before Stage 5; `submit_batch` fails
//! closed until AI-P04 replaces the current mock method.

use std::fmt;

#[cfg(test)]
use std::sync::Arc;

/// Hex-encoded 32-byte transaction id (`0x` + 64 hex chars).
pub type TxHash = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletError {
    InvalidAddress(String),
    InvalidAmount(String),
    NotFound(String),
    Rejected(String),
    NotImplemented(&'static str),
    Backend(String),
}

impl fmt::Display for WalletError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAddress(msg) => write!(f, "invalid address: {msg}"),
            Self::InvalidAmount(msg) => write!(f, "invalid amount: {msg}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::Rejected(msg) => write!(f, "rejected: {msg}"),
            Self::NotImplemented(what) => write!(f, "{what} is not implemented yet"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
        }
    }
}

impl std::error::Error for WalletError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxStatus {
    Success,
    Pending,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxRecord {
    pub txid: TxHash,
    pub kind: String,
    pub from: String,
    pub to: Option<String>,
    pub amount: u64,
    pub fee: u64,
    pub status: TxStatus,
    pub height: Option<u64>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiMethod {
    pub name: String,
    pub inputs: Vec<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractRecord {
    pub address: String,
    pub name: String,
    pub abi: String,
    pub methods: Vec<AbiMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCall {
    pub to: String,
    pub amount: u64,
    pub calldata: Vec<u8>,
}

/// Backend contract used by every wallet screen tab.
///
/// All methods are synchronous: egui's `App::update` is synchronous and the
/// in-process adapter performs point-gets on the storage engine. Network
/// clients can wrap a blocking JSON-RPC client behind the same trait.
pub trait WalletBackend: Send + Sync {
    fn wallet_address(&self) -> String;
    fn validate_address(&self, address: &str) -> bool;

    /// `eth_getBalance`
    fn get_balance(&self, address: &str) -> Result<u64, WalletError>;

    /// `agent_estimateTaskCost` seam. Fixed zero until Stage 5.
    fn estimate_fee(&self) -> u64 {
        0
    }

    /// Recent transactions involving `address`, newest first.
    fn recent_transactions(
        &self,
        address: &str,
        limit: usize,
    ) -> Result<Vec<TxRecord>, WalletError>;

    /// `eth_getTransactionByHash`
    fn get_transaction(&self, txid: &str) -> Result<Option<TxRecord>, WalletError>;

    fn deployed_contracts(&self) -> Result<Vec<ContractRecord>, WalletError>;

    /// `eth_call` — read-only VM execution, never mutates state.
    fn call_contract(
        &self,
        address: &str,
        method: &str,
        args: &[String],
    ) -> Result<Vec<u8>, WalletError>;

    /// `eth_sendTransaction` — transfer; the adapter owns the signing key.
    fn submit_transaction(&self, to: &str, amount: u64, fee: u64) -> Result<TxHash, WalletError>;

    /// Contract write (`eth_sendTransaction` with calldata).
    fn submit_call(
        &self,
        to: &str,
        method: &str,
        args: &[String],
        value: u64,
    ) -> Result<TxHash, WalletError>;

    /// `agent_signAndSubmitBatch`. Explicitly unavailable until AI-P04.
    fn submit_batch(&self, _calls: &[BatchCall]) -> Result<Vec<TxHash>, WalletError> {
        Err(WalletError::NotImplemented(
            "batch send (agent_signAndSubmitBatch) is available after AI-P04",
        ))
    }
}

/// Deterministic in-memory backend for UI tests, snapshots and the gallery.
pub struct MockWalletBackend {
    address: String,
    balance: u64,
    transactions: Vec<TxRecord>,
    contracts: Vec<ContractRecord>,
    last_error: Option<WalletError>,
    next_txid: u64,
}

impl MockWalletBackend {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            balance: 1000,
            transactions: Vec::new(),
            contracts: Vec::new(),
            last_error: None,
            next_txid: 1,
        }
    }

    pub fn with_balance(mut self, balance: u64) -> Self {
        self.balance = balance;
        self
    }

    pub fn with_transactions(mut self, transactions: Vec<TxRecord>) -> Self {
        self.transactions = transactions;
        self
    }

    pub fn with_contracts(mut self, contracts: Vec<ContractRecord>) -> Self {
        self.contracts = contracts;
        self
    }

    pub fn with_error(mut self, error: WalletError) -> Self {
        self.last_error = Some(error);
        self
    }

    pub fn transactions(&self) -> &[TxRecord] {
        &self.transactions
    }
}

impl WalletBackend for MockWalletBackend {
    fn wallet_address(&self) -> String {
        self.address.clone()
    }

    fn validate_address(&self, address: &str) -> bool {
        !address.trim().is_empty()
    }

    fn get_balance(&self, _address: &str) -> Result<u64, WalletError> {
        if let Some(err) = &self.last_error {
            return Err(err.clone());
        }
        Ok(self.balance)
    }

    fn recent_transactions(
        &self,
        _address: &str,
        limit: usize,
    ) -> Result<Vec<TxRecord>, WalletError> {
        Ok(self.transactions.iter().take(limit).cloned().collect())
    }

    fn get_transaction(&self, txid: &str) -> Result<Option<TxRecord>, WalletError> {
        Ok(self.transactions.iter().find(|tx| tx.txid == txid).cloned())
    }

    fn deployed_contracts(&self) -> Result<Vec<ContractRecord>, WalletError> {
        Ok(self.contracts.clone())
    }

    fn call_contract(
        &self,
        _address: &str,
        _method: &str,
        _args: &[String],
    ) -> Result<Vec<u8>, WalletError> {
        Ok(Vec::new())
    }

    fn submit_transaction(
        &self,
        _to: &str,
        _amount: u64,
        _fee: u64,
    ) -> Result<TxHash, WalletError> {
        if let Some(err) = &self.last_error {
            return Err(err.clone());
        }
        Ok(format!("0x{:064x}", self.next_txid))
    }

    fn submit_call(
        &self,
        _to: &str,
        _method: &str,
        _args: &[String],
        _value: u64,
    ) -> Result<TxHash, WalletError> {
        Ok(format!("0x{:064x}", self.next_txid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_backend_is_deterministic_and_fails_batch_closed() {
        let backend = MockWalletBackend::new("sc1test");
        assert_eq!(backend.wallet_address(), "sc1test");
        assert_eq!(backend.get_balance("sc1test"), Ok(1000));
        assert_eq!(backend.estimate_fee(), 0);
        assert!(backend
            .submit_batch(&[])
            .unwrap_err()
            .to_string()
            .contains("AI-P04"));
    }

    #[test]
    fn shared_backend_type_erasure_works() {
        let backend: Arc<dyn WalletBackend> = Arc::new(MockWalletBackend::new("sc1test"));
        assert!(backend.validate_address("sc1test"));
        assert_eq!(
            backend.submit_transaction("sc1other", 1, 0).unwrap().len(),
            66
        );
    }
}
