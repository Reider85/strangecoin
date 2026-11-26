//! Stage-4 wallet API edge (UI-P03, ADR-0031 Amendment C).
//!
//! `src/api/` was a deliberately empty Stage-0 placeholder (BUG-S0-020).
//! UI-P03 fills it with the in-process adapter behind `strangecoin_ui::WalletBackend`.
//! The names mirror `eth_*` JSON-RPC methods so an HTTP client can replace this
//! adapter later without touching the screen layer; no HTTP server or new
//! dependency is introduced here.

#![cfg(feature = "ui")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use strangecoin_core::address::decode_address;
use strangecoin_core::serialize::{deserialize_block, txid};
use strangecoin_core::types::Transaction;
use strangecoin_core::vm::gas::BLOCK_GAS_LIMIT;
use strangecoin_ui::{
    AbiMethod, ContractRecord, TxHash, TxRecord, TxStatus, WalletBackend, WalletError,
};

use crate::storage::{block_by_height_key, Cf};
use crate::{wallet, BlockchainFacade};

/// Mutable wallet identity shared with the strangler launcher.
#[derive(Debug, Default)]
pub struct WalletSession {
    pub address: String,
    pub password: String,
}

/// Optional mining dispatch performed after a signed tx is admitted.
pub type SubmitHook = Arc<dyn Fn(&Transaction) + Send + Sync>;

/// In-process implementation of the wallet data-source seam.
pub struct InProcessWalletBackend {
    blockchain: Arc<BlockchainFacade>,
    network_id: u32,
    data_dir: PathBuf,
    session: Arc<Mutex<WalletSession>>,
    submit_hook: Option<SubmitHook>,
}

impl InProcessWalletBackend {
    pub fn new(blockchain: Arc<BlockchainFacade>, network_id: u32, data_dir: PathBuf) -> Self {
        Self {
            blockchain,
            network_id,
            data_dir,
            session: Arc::new(Mutex::new(WalletSession::default())),
            submit_hook: None,
        }
    }

    pub fn session(&self) -> Arc<Mutex<WalletSession>> {
        Arc::clone(&self.session)
    }

    pub fn with_submit_hook(mut self, hook: SubmitHook) -> Self {
        self.submit_hook = Some(hook);
        self
    }

    fn session_address(&self) -> String {
        self.session
            .lock()
            .map(|s| s.address.clone())
            .unwrap_or_default()
    }

    fn session_password(&self) -> Result<String, WalletError> {
        self.session
            .lock()
            .map(|s| s.password.clone())
            .map_err(|_| WalletError::Backend("wallet session lock poisoned".into()))
    }

    fn record(&self, tx: &Transaction, height: Option<u64>, timestamp: u64) -> TxRecord {
        let status = match height {
            Some(_) => TxStatus::Success,
            None => TxStatus::Pending,
        };
        TxRecord {
            txid: format!("0x{}", hex::encode(txid(tx))),
            kind: match tx {
                Transaction::Transfer(_) => "transfer".to_string(),
                Transaction::Create(_) => "create".to_string(),
                Transaction::Call(_) => "call".to_string(),
            },
            from: tx.sender().to_string(),
            to: tx.receiver().map(str::to_string),
            amount: tx.value(),
            fee: tx.fee(),
            status,
            height,
            timestamp,
        }
    }

    fn sign_and_admit(&self, mut tx: Transaction) -> Result<TxHash, WalletError> {
        let address = self.session_address();
        let password = self.session_password()?;
        // Keystore filenames are derived from the base64 public key, not the
        // bech32 address (src/wallet.rs). Resolve by the loaded key's address
        // instead of guessing a filename — also tolerates older keystores.
        let keystores = wallet::Wallet::list_keystores(&self.data_dir)
            .map_err(|err| WalletError::Backend(format!("keystore scan failed: {err}")))?;
        let wallet = keystores
            .iter()
            .find_map(|path| {
                let loaded = wallet::Wallet::load(&password, path).ok()?;
                let encoded =
                    strangecoin_core::address::encode_address(&loaded.public_key, self.network_id)
                        .ok()?;
                (encoded == address).then_some(loaded)
            })
            .ok_or_else(|| WalletError::Backend(format!("no keystore for `{address}`")))?;
        wallet
            .sign_transaction(&mut tx)
            .map_err(|err| WalletError::Backend(format!("signing failed: {err}")))?;
        self.blockchain
            .apply_tx(tx.clone())
            .map_err(|err| WalletError::Rejected(err.to_string()))?;
        if let Some(hook) = &self.submit_hook {
            hook(&tx);
        }
        Ok(format!("0x{}", hex::encode(txid(&tx))))
    }

    fn calldata(method: &str, args: &[String]) -> Vec<u8> {
        let mut data = blake3::hash(method.as_bytes()).as_bytes()[..4].to_vec();
        for arg in args {
            data.extend_from_slice(arg.as_bytes());
        }
        data
    }
}

impl WalletBackend for InProcessWalletBackend {
    fn wallet_address(&self) -> String {
        self.session_address()
    }

    fn validate_address(&self, address: &str) -> bool {
        match decode_address(address.trim()) {
            Ok((_, network_id)) => network_id == self.network_id,
            Err(_) => false,
        }
    }

    fn get_balance(&self, address: &str) -> Result<u64, WalletError> {
        Ok(self.blockchain.get_balance(address))
    }

    fn recent_transactions(
        &self,
        address: &str,
        limit: usize,
    ) -> Result<Vec<TxRecord>, WalletError> {
        let mut out = Vec::new();
        for tx in self.blockchain.mempool_transactions() {
            if tx.sender() == address || tx.receiver() == Some(address) {
                out.push(self.record(&tx, None, 0));
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }

        let engine = self.blockchain.storage_engine();
        let tip = self.blockchain.tip().map(|b| b.index).unwrap_or(0);
        for height in (0..=tip).rev() {
            let raw = engine
                .get(Cf::Blocks, &block_by_height_key(height))
                .ok()
                .flatten();
            let Some(raw) = raw else { continue };
            let Ok(block) = deserialize_block(&raw) else {
                continue;
            };
            for tx in block.transactions.iter().rev() {
                if tx.sender() == address || tx.receiver() == Some(address) {
                    out.push(self.record(tx, Some(height), block.timestamp));
                    if out.len() >= limit {
                        return Ok(out);
                    }
                }
            }
        }
        Ok(out)
    }

    fn get_transaction(&self, txid_hex: &str) -> Result<Option<TxRecord>, WalletError> {
        let bytes = hex::decode(txid_hex.strip_prefix("0x").unwrap_or(txid_hex))
            .map_err(|err| WalletError::Backend(format!("bad txid hex: {err}")))?;
        let id: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| WalletError::Backend("txid must be 32 bytes".into()))?;
        let Some(tx) = self.blockchain.get_tx(&id) else {
            return Ok(None);
        };
        let height = self.blockchain.tx_location(&id).map(|(h, _)| h);
        let timestamp = height
            .and_then(|h| {
                self.blockchain
                    .storage_engine()
                    .get(Cf::Blocks, &block_by_height_key(h))
                    .ok()
                    .flatten()
            })
            .and_then(|raw| deserialize_block(&raw).ok())
            .map(|block| block.timestamp)
            .unwrap_or(0);
        Ok(Some(self.record(&tx, height, timestamp)))
    }

    fn deployed_contracts(&self) -> Result<Vec<ContractRecord>, WalletError> {
        let mut contracts = Vec::new();
        for address in self.blockchain.account_keys() {
            let Some(account) = self.blockchain.get_account(&address) else {
                continue;
            };
            if !account.is_contract() {
                continue;
            }
            // No indexer yet (S4-core-P07): ABI cannot be recovered from the
            // chain. Offer the two generic entry points so Read/Write remain
            // usable, with an honest empty ABI viewer body.
            contracts.push(ContractRecord {
                name: format!("Contract {}", &address.chars().take(10).collect::<String>()),
                address,
                abi: String::new(),
                methods: vec![
                    AbiMethod {
                        name: "call".to_string(),
                        inputs: Vec::new(),
                        read_only: true,
                    },
                    AbiMethod {
                        name: "send".to_string(),
                        inputs: Vec::new(),
                        read_only: false,
                    },
                ],
            });
        }
        Ok(contracts)
    }

    fn call_contract(
        &self,
        address: &str,
        method: &str,
        args: &[String],
    ) -> Result<Vec<u8>, WalletError> {
        let data = Self::calldata(method, args);
        crate::blockchain::read_only::read_only_call(
            &self.blockchain,
            address,
            data,
            BLOCK_GAS_LIMIT / 4,
        )
        .map(|result| result.return_data)
        .map_err(|err| WalletError::Backend(format!("{err:?}")))
    }

    fn submit_transaction(&self, to: &str, amount: u64, fee: u64) -> Result<TxHash, WalletError> {
        if !self.validate_address(to) {
            return Err(WalletError::InvalidAddress(to.to_string()));
        }
        if amount == 0 {
            return Err(WalletError::InvalidAmount("amount must be positive".into()));
        }
        let from = self.session_address();
        if from.is_empty() {
            return Err(WalletError::Backend("wallet is not unlocked".into()));
        }
        let nonce = self.blockchain.get_nonce(&from).saturating_add(1);
        let mut tx = Transaction::transfer(&from, to.trim(), amount, nonce, self.network_id);
        if let Transaction::Transfer(transfer) = &mut tx {
            transfer.fee = fee;
        }
        self.sign_and_admit(tx)
    }

    fn submit_call(
        &self,
        to: &str,
        method: &str,
        args: &[String],
        value: u64,
    ) -> Result<TxHash, WalletError> {
        if !self.validate_address(to) {
            return Err(WalletError::InvalidAddress(to.to_string()));
        }
        let from = self.session_address();
        if from.is_empty() {
            return Err(WalletError::Backend("wallet is not unlocked".into()));
        }
        let nonce = self.blockchain.get_nonce(&from).saturating_add(1);
        let tx = Transaction::call(
            &from,
            to.trim(),
            Self::calldata(method, args),
            value,
            BLOCK_GAS_LIMIT / 4,
            nonce,
            self.network_id,
        );
        self.sign_and_admit(tx)
    }
}

// ---------------------------------------------------------------------------
// UI-P04: in-process dashboard / transaction-audit adapters.
// ---------------------------------------------------------------------------

use crate::events::NodeEvent;
use std::collections::HashSet;
use strangecoin_core::vm::Receipt;
use strangecoin_core::{serialize_state_witness, StateWitness, Transaction as CoreTransaction};
use strangecoin_net::PeerManager;
use strangecoin_ui::{
    BlockRow, DashboardBackend, DashboardSnapshot, MempoolRow, PeerRow, SyncStatus,
    TxDetailBackend, TxDetailData, TxEventLog, TxOverview, TxReceipt, TxVerifyResult, WitnessProof,
};

/// Bounded, deterministic summary of the `NodeEvent` stream. The launcher
/// drains its `EventBus` receiver into this digest; the dashboard renders the
/// digest instead of consuming wall-clock timestamps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EventDigest {
    pub blocks_applied: u64,
    pub reorgs: u64,
    pub tx_accepted: u64,
    pub tx_rejected: u64,
    pub peer_score_changes: u64,
    pub state_persisted: u64,
    pub last_height: u64,
}

impl EventDigest {
    pub fn apply(&mut self, event: &NodeEvent) {
        match event {
            NodeEvent::BlockApplied { height, .. } => {
                self.blocks_applied += 1;
                self.last_height = *height;
            }
            NodeEvent::BlockReorged { .. } => self.reorgs += 1,
            NodeEvent::TxAccepted { .. } => self.tx_accepted += 1,
            NodeEvent::TxRejected { .. } => self.tx_rejected += 1,
            NodeEvent::PeerScoreChanged { .. } => self.peer_score_changes += 1,
            NodeEvent::StatePersisted { height } => {
                self.state_persisted += 1;
                self.last_height = *height;
            }
            NodeEvent::MiningStarted | NodeEvent::MiningFinished => {}
        }
    }
}

/// In-process implementation of `DashboardBackend` (ADR-0031 Amendment D).
/// Snapshots are pull-based; every value comes from node state, never from a
/// fabricated fixture. Fields that the chain does not expose (gas used, RBF
/// conflicts, queue depths, HELLO capabilities) are reported as empty/zero.
pub struct InProcessDashboardBackend {
    blockchain: Arc<BlockchainFacade>,
    peer_manager: Arc<PeerManager>,
    digest: Arc<Mutex<EventDigest>>,
}

impl InProcessDashboardBackend {
    pub fn new(blockchain: Arc<BlockchainFacade>, peer_manager: Arc<PeerManager>) -> Self {
        Self {
            blockchain,
            peer_manager,
            digest: Arc::new(Mutex::new(EventDigest::default())),
        }
    }

    pub fn digest_handle(&self) -> Arc<Mutex<EventDigest>> {
        Arc::clone(&self.digest)
    }

    pub fn record_event(&self, event: &NodeEvent) {
        if let Ok(mut digest) = self.digest.lock() {
            digest.apply(event);
        }
    }
}

fn hex_txid(tx: &CoreTransaction) -> String {
    format!("0x{}", hex::encode(txid(tx)))
}

impl DashboardBackend for InProcessDashboardBackend {
    fn snapshot(&self) -> DashboardSnapshot {
        let chain = self.blockchain.chain_snapshot();
        let height = chain.last().map(|block| block.index).unwrap_or(0);
        let connected = self.peer_manager.connected_peers();
        let connected_set: HashSet<_> = connected.into_iter().collect();

        let peers: Vec<PeerRow> = self
            .peer_manager
            .all_peers_sorted()
            .into_iter()
            .map(|record| PeerRow {
                node_id: record.node_id.to_string(),
                addr: record.addr.to_string(),
                latency_ms: record.latency_ewma.as_millis() as u64,
                score: record.score,
                direction: if connected_set.contains(&record.node_id) {
                    "connected"
                } else {
                    "known"
                }
                .to_string(),
                capabilities: Vec::new(),
            })
            .collect();

        let mempool = self.blockchain.mempool_transactions();
        let total_fee = mempool.iter().map(|tx| tx.fee()).sum();
        let mempool_rows: Vec<MempoolRow> = mempool
            .iter()
            .map(|tx| MempoolRow {
                txid: hex_txid(tx),
                sender: tx.sender().to_string(),
                receiver: tx.receiver().unwrap_or("").to_string(),
                amount: tx.value(),
                fee: tx.fee(),
                gas: 0,
                rbf_conflict: false,
            })
            .collect();

        let blocks: Vec<BlockRow> = chain
            .iter()
            .rev()
            .take(10)
            .map(|block| BlockRow {
                height: block.index,
                hash: block.hash.clone(),
                tx_count: block.transactions.len(),
                gas_used: 0,
                miner: block
                    .transactions
                    .iter()
                    .find(|tx| tx.is_coinbase())
                    .and_then(|tx| tx.receiver())
                    .unwrap_or("")
                    .to_string(),
                timestamp: block.timestamp,
                txids: block.transactions.iter().map(hex_txid).collect(),
            })
            .collect();

        let digest = self
            .digest
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let state = if digest.reorgs > 0 {
            "catch-up"
        } else if digest.blocks_applied > 0 {
            "blocks"
        } else {
            "idle"
        };

        DashboardSnapshot {
            peers,
            sync: SyncStatus {
                current_height: height,
                tip_height: height,
                sync_percent: 100,
                headers_queue: 0,
                blocks_queue: 0,
                state: state.to_string(),
            },
            mempool: mempool_rows,
            mempool_count: self.blockchain.mempool_len(),
            blocks,
            total_fee,
        }
    }
}

fn parse_txid(txid: &str) -> Result<[u8; 32], WalletError> {
    let stripped = txid.strip_prefix("0x").unwrap_or(txid);
    let bytes = hex::decode(stripped)
        .map_err(|_| WalletError::InvalidAddress(format!("invalid txid `{txid}`")))?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| WalletError::InvalidAddress(format!("txid must be 32 bytes: `{txid}`")))?;
    Ok(array)
}

fn ui_status(status: strangecoin_core::vm::TxStatus) -> strangecoin_ui::TxStatus {
    match status {
        strangecoin_core::vm::TxStatus::Success => strangecoin_ui::TxStatus::Success,
        strangecoin_core::vm::TxStatus::Reverted
        | strangecoin_core::vm::TxStatus::OutOfGas
        | strangecoin_core::vm::TxStatus::Failed => strangecoin_ui::TxStatus::Failed,
    }
}

fn tx_events(receipt: &Receipt) -> Vec<TxEventLog> {
    receipt
        .logs
        .iter()
        .map(|log| TxEventLog {
            address: log.address.clone(),
            topics: log
                .topics
                .iter()
                .map(|topic| format!("0x{}", hex::encode(topic)))
                .collect(),
            data: format!("0x{}", hex::encode(&log.data)),
        })
        .collect()
}

/// In-process implementation of `TxDetailBackend` (ADR-0031 Amendment D).
/// Receipts are tip-only and derived; witnesses are block-level and built
/// lazily in `verify` because `build_witness_for_block` replays the chain.
pub struct InProcessTxDetailBackend {
    blockchain: Arc<BlockchainFacade>,
}

impl InProcessTxDetailBackend {
    pub fn new(blockchain: Arc<BlockchainFacade>) -> Self {
        Self { blockchain }
    }

    fn witness_proof(&self, witness: &StateWitness) -> WitnessProof {
        let mut proofs: Vec<_> = witness.proofs.iter().collect();
        proofs.sort_by(|(left, _), (right, _)| left.cmp(right));
        let account_proofs = proofs
            .iter()
            .map(|(address, proof)| {
                format!(
                    "{address}: balance={} nonce={} path={}",
                    proof.balance,
                    proof.nonce,
                    proof
                        .proof
                        .iter()
                        .map(|node| hex::encode(node))
                        .collect::<Vec<_>>()
                        .join("")
                )
            })
            .collect();
        let storage_proofs = proofs
            .iter()
            .filter_map(|(address, proof)| {
                proof
                    .storage_root
                    .map(|root| format!("{address}: storage_root=0x{}", hex::encode(root)))
            })
            .collect();
        WitnessProof {
            pre_state_root: format!("0x{}", hex::encode(witness.pre_state_root)),
            account_proofs,
            storage_proofs,
            raw: format!("0x{}", hex::encode(serialize_state_witness(witness))),
            available: true,
        }
    }
}

impl TxDetailBackend for InProcessTxDetailBackend {
    fn load(&self, txid: &str) -> Result<TxDetailData, WalletError> {
        let id = parse_txid(txid)?;
        let location = self.blockchain.tx_location(&id);
        let transaction = self
            .blockchain
            .get_tx(&id)
            .or_else(|| self.blockchain.mempool_tx(&id))
            .ok_or_else(|| WalletError::NotFound(txid.to_string()))?;

        let chain = self.blockchain.chain_snapshot();
        let timestamp = location
            .and_then(|(height, _)| chain.iter().find(|block| block.index == height))
            .map(|block| block.timestamp)
            .unwrap_or(0);
        let input = match &transaction {
            CoreTransaction::Call(call) => format!("0x{}", hex::encode(&call.data)),
            _ => "0x".to_string(),
        };
        let overview = TxOverview {
            txid: hex_txid(&transaction),
            kind: match transaction {
                CoreTransaction::Transfer(_) => "transfer",
                CoreTransaction::Create(_) => "create",
                CoreTransaction::Call(_) => "call",
            }
            .to_string(),
            from: transaction.sender().to_string(),
            to: transaction.receiver().unwrap_or("").to_string(),
            amount: transaction.value(),
            fee: transaction.fee(),
            gas_used: 0,
            status: if location.is_some() {
                strangecoin_ui::TxStatus::Success
            } else {
                strangecoin_ui::TxStatus::Pending
            },
            block_height: location.map(|(height, _)| height),
            timestamp,
            input,
        };

        let receipt = self
            .blockchain
            .tip_receipts()
            .into_iter()
            .find(|candidate| candidate.txid == id);
        let (receipt_data, events) = match receipt {
            Some(receipt) => {
                let events = tx_events(&receipt);
                (
                    TxReceipt {
                        status: ui_status(receipt.status),
                        gas_used: receipt.gas_used,
                        gas_limit: 0,
                        logs: events.clone(),
                        revert_reason: (receipt.status != strangecoin_core::vm::TxStatus::Success)
                            .then(|| format!("{:?}", receipt.status)),
                        available: true,
                    },
                    events,
                )
            }
            None => (
                TxReceipt {
                    status: strangecoin_ui::TxStatus::Pending,
                    gas_used: 0,
                    gas_limit: 0,
                    logs: Vec::new(),
                    revert_reason: None,
                    available: false,
                },
                Vec::new(),
            ),
        };

        Ok(TxDetailData {
            overview: Some(overview),
            receipt: Some(receipt_data),
            witness: None,
            events,
        })
    }

    fn verify(&self, txid: &str) -> Result<TxVerifyResult, WalletError> {
        let id = parse_txid(txid)?;
        let (height, _) = self
            .blockchain
            .tx_location(&id)
            .ok_or_else(|| WalletError::Rejected("transaction is not confirmed".to_string()))?;
        let witness = self
            .blockchain
            .build_witness_for_block(height)
            .map_err(|err| WalletError::Backend(err.to_string()))?;
        let chain = self.blockchain.chain_snapshot();
        let base = chain.first().map(|block| block.index).unwrap_or(0);
        let index = height
            .checked_sub(base)
            .ok_or_else(|| WalletError::Backend("witness height precedes chain base".to_string()))?
            as usize;
        let block = chain
            .get(index)
            .ok_or_else(|| WalletError::Backend("witness height out of range".to_string()))?;
        let proof = self.witness_proof(&witness);
        let (verified, message) = match self
            .blockchain
            .verify_incoming_block_with_witness(block, &witness)
        {
            Ok(()) => (true, "verified".to_string()),
            Err(err) => (false, err.to_string()),
        };
        Ok(TxVerifyResult {
            verified,
            message,
            witness: Some(proof),
        })
    }
}

#[cfg(test)]
mod ui_p04_tests {
    use super::*;

    #[test]
    fn event_digest_is_deterministic_and_bounded() {
        let mut digest = EventDigest::default();
        digest.apply(&NodeEvent::BlockApplied {
            height: 7,
            hash: "0xabc".to_string(),
        });
        digest.apply(&NodeEvent::TxAccepted {
            txid: "0x01".to_string(),
        });
        assert_eq!(digest.blocks_applied, 1);
        assert_eq!(digest.tx_accepted, 1);
        assert_eq!(digest.last_height, 7);
    }

    #[test]
    fn txid_parser_fails_closed() {
        assert!(parse_txid("0x1234").is_err());
        assert!(parse_txid("not-hex").is_err());
        assert_eq!(
            parse_txid(&format!("0x{}", "ab".repeat(32))).unwrap().len(),
            32
        );
    }
}
