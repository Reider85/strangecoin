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
