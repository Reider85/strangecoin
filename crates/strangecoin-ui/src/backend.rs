//! Wallet / node data-source seam (UI-P03 + UI-P04, ADR-0031 Amendments C/D).
//!
//! The screen layer must not depend on the node monolith (that would invert
//! the strangler direction). `WalletBackend`, `DashboardBackend` and
//! `TxDetailBackend` are implemented in the node's `src/api/` module over
//! `BlockchainFacade` / `PeerManager`; deterministic mocks are used by UI unit
//! tests, the a11y gate and the gallery.
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

// ---------------------------------------------------------------------------
// UI-P04: node dashboard + transaction detail data-source seams.
// ---------------------------------------------------------------------------

/// One row of the Peers panel (ADR-0022 scoring bands are rendered by the
/// screen, not by the backend).
#[derive(Debug, Clone, PartialEq)]
pub struct PeerRow {
    pub node_id: String,
    pub addr: String,
    pub latency_ms: u64,
    pub score: f32,
    pub direction: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    pub current_height: u64,
    pub tip_height: u64,
    /// Integer percentage (0..=100) — no floating-point rounding in state.
    pub sync_percent: u64,
    pub headers_queue: usize,
    pub blocks_queue: usize,
    /// `idle` / `headers-first` / `blocks` / `catch-up`.
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MempoolRow {
    pub txid: String,
    pub sender: String,
    pub receiver: String,
    pub amount: u64,
    pub fee: u64,
    pub gas: u64,
    pub rbf_conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRow {
    pub height: u64,
    pub hash: String,
    pub tx_count: usize,
    pub gas_used: u64,
    pub miner: String,
    pub timestamp: u64,
    /// Header + tx list for the block detail modal.
    pub txids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardSnapshot {
    pub peers: Vec<PeerRow>,
    pub sync: SyncStatus,
    pub mempool: Vec<MempoolRow>,
    /// Authoritative mempool size (rows are capped to the top 10).
    pub mempool_count: usize,
    pub blocks: Vec<BlockRow>,
    pub total_fee: u64,
}

/// Pull-based seam: the launcher calls `snapshot()` once per rendered frame.
/// It stays synchronous because egui's `App::update` is synchronous and the
/// in-process adapter reads node state through point-gets / lock-free clones.
pub trait DashboardBackend: Send + Sync {
    fn snapshot(&self) -> DashboardSnapshot;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxEventLog {
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxOverview {
    pub txid: String,
    pub kind: String,
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub gas_used: u64,
    pub status: TxStatus,
    pub block_height: Option<u64>,
    pub timestamp: u64,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxReceipt {
    pub status: TxStatus,
    pub gas_used: u64,
    pub gas_limit: u64,
    pub logs: Vec<TxEventLog>,
    pub revert_reason: Option<String>,
    /// Receipts are derived and only retained for the tip block (ADR-0024).
    /// `false` means "unavailable", never fabricated.
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessProof {
    pub pre_state_root: String,
    pub account_proofs: Vec<String>,
    pub storage_proofs: Vec<String>,
    /// Hex encoding of the canonical `StateWitness` wire codec (S3-P11).
    pub raw: String,
    /// A witness is block-level; `false` means none could be reconstructed.
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxDetailData {
    pub overview: Option<TxOverview>,
    pub receipt: Option<TxReceipt>,
    pub witness: Option<WitnessProof>,
    pub events: Vec<TxEventLog>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxVerifyResult {
    pub verified: bool,
    pub message: String,
    /// The proof is built on demand (O(height) replay) and returned with the
    /// verification result — never eagerly on screen open.
    pub witness: Option<WitnessProof>,
}

/// Transaction audit seam. `verify` is lazy: the screen only calls it after
/// an explicit button press because witness reconstruction is an O(height)
/// replay (S3-P11 documented limitation).
pub trait TxDetailBackend: Send + Sync {
    fn load(&self, txid: &str) -> Result<TxDetailData, WalletError>;
    fn verify(&self, txid: &str) -> Result<TxVerifyResult, WalletError>;
}

/// Deterministic dashboard fixture (no wall clock, no RNG).
pub struct MockDashboardBackend {
    snapshot: DashboardSnapshot,
}

impl MockDashboardBackend {
    pub fn new() -> Self {
        Self {
            snapshot: DashboardSnapshot {
                peers: vec![
                    PeerRow {
                        node_id: "peer-a".to_string(),
                        addr: "127.0.0.1:8333".to_string(),
                        latency_ms: 12,
                        score: 0.82,
                        direction: "outbound".to_string(),
                        capabilities: vec!["Noise".to_string(), "Erlay".to_string()],
                    },
                    PeerRow {
                        node_id: "peer-b".to_string(),
                        addr: "127.0.0.1:8334".to_string(),
                        latency_ms: 320,
                        score: 0.35,
                        direction: "inbound".to_string(),
                        capabilities: vec!["Noise".to_string()],
                    },
                    PeerRow {
                        node_id: "peer-c".to_string(),
                        addr: "127.0.0.1:8335".to_string(),
                        latency_ms: 900,
                        score: 0.05,
                        direction: "inbound".to_string(),
                        capabilities: Vec::new(),
                    },
                ],
                sync: SyncStatus {
                    current_height: 42,
                    tip_height: 42,
                    sync_percent: 100,
                    headers_queue: 0,
                    blocks_queue: 0,
                    state: "idle".to_string(),
                },
                mempool: vec![
                    MempoolRow {
                        txid: "0x0000000000000000000000000000000000000000000000000000000000000001"
                            .to_string(),
                        sender: "sc1sender".to_string(),
                        receiver: "sc1receiver".to_string(),
                        amount: 100,
                        fee: 7,
                        gas: 21000,
                        rbf_conflict: true,
                    },
                    MempoolRow {
                        txid: "0x0000000000000000000000000000000000000000000000000000000000000002"
                            .to_string(),
                        sender: "sc1sender".to_string(),
                        receiver: "sc1receiver".to_string(),
                        amount: 50,
                        fee: 3,
                        gas: 21000,
                        rbf_conflict: false,
                    },
                ],
                mempool_count: 2,
                blocks: (0..10)
                    .map(|index| BlockRow {
                        height: 42 - index,
                        hash: format!("block-{:02}", 42 - index),
                        tx_count: 2,
                        gas_used: 42000,
                        miner: "sc1miner".to_string(),
                        timestamp: 1_700_000_000 + (42 - index) * 60,
                        txids: vec![format!("0x{:064x}", 42 - index)],
                    })
                    .collect(),
                total_fee: 10,
            },
        }
    }

    pub fn with_snapshot(mut self, snapshot: DashboardSnapshot) -> Self {
        self.snapshot = snapshot;
        self
    }
}

impl Default for MockDashboardBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardBackend for MockDashboardBackend {
    fn snapshot(&self) -> DashboardSnapshot {
        self.snapshot.clone()
    }
}

/// Deterministic transaction-detail fixture for the a11y gate and gallery.
pub struct MockTxDetailBackend {
    data: Vec<TxDetailData>,
    verify_result: TxVerifyResult,
}

impl MockTxDetailBackend {
    pub fn new() -> Self {
        let txid = "0x0000000000000000000000000000000000000000000000000000000000000001";
        Self {
            data: vec![TxDetailData {
                overview: Some(TxOverview {
                    txid: txid.to_string(),
                    kind: "transfer".to_string(),
                    from: "sc1sender".to_string(),
                    to: "sc1receiver".to_string(),
                    amount: 100,
                    fee: 7,
                    gas_used: 21000,
                    status: TxStatus::Success,
                    block_height: Some(42),
                    timestamp: 1_700_000_000,
                    input: "0x".to_string(),
                }),
                receipt: Some(TxReceipt {
                    status: TxStatus::Success,
                    gas_used: 21000,
                    gas_limit: 30000,
                    logs: vec![TxEventLog {
                        address: "sc1contract".to_string(),
                        topics: vec!["0x01".to_string()],
                        data: "0xdeadbeef".to_string(),
                    }],
                    revert_reason: None,
                    available: true,
                }),
                witness: Some(WitnessProof {
                    pre_state_root: "0x00".to_string(),
                    account_proofs: vec!["0xabcd".to_string()],
                    storage_proofs: vec!["0xef01".to_string()],
                    raw: "0x00".to_string(),
                    available: true,
                }),
                events: vec![TxEventLog {
                    address: "sc1contract".to_string(),
                    topics: vec!["0x01".to_string()],
                    data: "0xdeadbeef".to_string(),
                }],
            }],
            verify_result: TxVerifyResult {
                verified: true,
                message: "verified".to_string(),
                witness: None,
            },
        }
    }

    pub fn with_data(mut self, data: Vec<TxDetailData>) -> Self {
        self.data = data;
        self
    }

    pub fn with_verify_result(mut self, result: TxVerifyResult) -> Self {
        self.verify_result = result;
        self
    }
}

impl Default for MockTxDetailBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl TxDetailBackend for MockTxDetailBackend {
    fn load(&self, txid: &str) -> Result<TxDetailData, WalletError> {
        self.data
            .iter()
            .find(|data| {
                data.overview
                    .as_ref()
                    .is_some_and(|overview| overview.txid == txid)
            })
            .cloned()
            .ok_or_else(|| WalletError::NotFound(txid.to_string()))
    }

    fn verify(&self, txid: &str) -> Result<TxVerifyResult, WalletError> {
        self.load(txid)?;
        Ok(self.verify_result.clone())
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

    #[test]
    fn dashboard_mock_is_deterministic_and_sorted_by_peer_id() {
        let backend: Arc<dyn DashboardBackend> = Arc::new(MockDashboardBackend::new());
        let first = backend.snapshot();
        let second = backend.snapshot();
        assert_eq!(first, second);
        assert_eq!(first.peers.len(), 3);
        assert_eq!(first.blocks.len(), 10);
        assert!(first.peers.windows(2).all(|w| w[0].node_id < w[1].node_id));
    }

    #[test]
    fn tx_detail_mock_is_deterministic_and_fails_closed_for_unknown_tx() {
        let backend: Arc<dyn TxDetailBackend> = Arc::new(MockTxDetailBackend::new());
        let txid = "0x0000000000000000000000000000000000000000000000000000000000000001";
        assert_eq!(backend.load(txid).unwrap(), backend.load(txid).unwrap());
        assert!(backend.verify(txid).unwrap().verified);
        assert!(matches!(
            backend.load("unknown"),
            Err(WalletError::NotFound(_))
        ));
        assert!(backend.verify("unknown").is_err());
    }
}
