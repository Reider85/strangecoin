//! `McpBackend` impl for [`BlockchainFacade`] (AI-P02, ADR-0029).
//!
//! Same strangler split as `strangecoin_net::BlockchainView`: the MCP crate
//! owns the protocol shell, this module wires it to the monolith — chain
//! lookups, mempool admission, the read-only VM path and event derivation.
//! Gate 3 of prompt-stage3.md §8.3 (business logic stays out of pure-core;
//! no consensus rules touched).

use strangecoin_core::serialize::{deserialize_block, txid};
use strangecoin_core::types::{Block, BlockHeader, Transaction};
use strangecoin_mcp::{CallOutcome, ContractInfo, McpBackend, McpError, McpEvent, TxRecord};

use crate::events::NodeEvent;
use crate::storage::{block_by_height_key, Cf};
use crate::BlockchainFacade;

impl McpBackend for BlockchainFacade {
    fn tip_header(&self) -> Option<BlockHeader> {
        self.tip().map(|block| block.header())
    }

    fn block_by_height(&self, height: u64) -> Option<Block> {
        // S3-P09 O(1) point-get: Blocks CF is keyed by BE height (ADR-0024).
        let raw = self
            .storage_engine()
            .get(Cf::Blocks, &block_by_height_key(height))
            .ok()
            .flatten()?;
        deserialize_block(&raw).ok()
    }

    fn block_by_hash(&self, hash_hex: &str) -> Option<Block> {
        BlockchainFacade::block_by_hash(self, hash_hex)
    }

    fn get_balance(&self, address: &str) -> u64 {
        BlockchainFacade::get_balance(self, address)
    }

    fn get_nonce(&self, address: &str) -> u64 {
        BlockchainFacade::get_nonce(self, address)
    }

    fn pending_balance(&self, address: &str) -> u64 {
        // Spec §2.2: confirmed adjusted by the mempool — subtract the
        // sender's pending (value + fee), add pending incoming value.
        // u64 addition is commutative, so iteration order cannot change the
        // result (#23).
        let mut delta = i128::from(self.get_balance(address));
        for tx in self.mempool_transactions() {
            if !tx.is_coinbase() && tx.sender() == address {
                delta -= i128::from(tx.value().saturating_add(tx.fee()));
            }
            if tx.receiver() == Some(address) {
                delta += i128::from(tx.value());
            }
        }
        delta.clamp(0, i128::from(u64::MAX)) as u64
    }

    fn submit_tx(&self, tx: Transaction) -> Result<[u8; 32], McpError> {
        let id = txid(&tx);
        self.apply_tx(tx)
            .map_err(|err| McpError::Rejected(err.to_string()))?;
        Ok(id)
    }

    fn call_contract(
        &self,
        address: &str,
        calldata: Vec<u8>,
        gas_limit: u64,
    ) -> Result<CallOutcome, McpError> {
        // UI-P03: the wallet's `eth_call` seam and this MCP tool share one
        // read-only execution path (src/blockchain/read_only.rs).
        let result = super::read_only::read_only_call(self, address, calldata, gas_limit).map_err(
            |err| match err {
                super::read_only::ReadOnlyError::NotFound(msg) => McpError::NotFound(msg),
                super::read_only::ReadOnlyError::Rejected(msg) => McpError::Rejected(msg),
            },
        )?;
        Ok(CallOutcome {
            gas_used: result.gas_used,
            return_data: result.return_data,
            logs: result.logs,
            status: result.status,
        })
    }

    fn get_tx_with_receipt(&self, txid_hex: &str) -> Result<TxRecord, McpError> {
        let bytes = hex::decode(txid_hex.strip_prefix("0x").unwrap_or(txid_hex))
            .map_err(|err| McpError::InvalidParams(format!("bad txid hex: {err}")))?;
        let id: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| McpError::InvalidParams("txid must be 32 bytes (64 hex chars)".into()))?;

        let tx = self
            .get_tx(&id)
            .ok_or_else(|| McpError::NotFound(format!("tx `{txid_hex}` not found")))?;
        let location = self.tx_location(&id);
        let height = location.map(|(height, _)| height);

        // Receipts are derived and only held for the tip block (ADR-0024 has
        // no receipts CF); older txs report `receipt: null` with the
        // confirmed `height` (documented in the spec amendment).
        let receipt = match (height, self.tip().map(|tip| tip.index)) {
            (Some(h), Some(tip)) if h == tip => {
                self.tip_receipts().into_iter().find(|r| r.txid == id)
            }
            _ => None,
        };

        Ok(TxRecord {
            tx,
            receipt,
            height,
        })
    }

    fn contract_info(&self, address: &str) -> Result<ContractInfo, McpError> {
        let account = self
            .get_account(address)
            .ok_or_else(|| McpError::NotFound(format!("no account `{address}`")))?;
        let contract = account
            .as_contract()
            .ok_or_else(|| McpError::NotFound(format!("`{address}` is not a contract account")))?;
        Ok(ContractInfo {
            code: self.with_inner(|bc| bc.codes.get(address).cloned()),
            storage_root: Some(contract.storage_root),
            abi_json: None,
        })
    }

    fn subscribe_events(&self) -> tokio::sync::mpsc::Receiver<McpEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let mut source = self.event_bus().subscribe_async(64);
        let facade = self.clone();
        tokio::spawn(async move {
            while let Some(event) = source.recv().await {
                let NodeEvent::BlockApplied { height, hash } = event else {
                    continue; // The MCP surface exposes the 3 spec §2.7 types only.
                };
                // Order: BlockApplied → TxConfirmed → EventLog (fixed, so
                // two nodes replaying the same block emit the same sequence).
                if tx
                    .send(McpEvent::BlockApplied {
                        height,
                        hash: hash.clone(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
                if let Some(block) = facade.block_by_height(height) {
                    for transaction in &block.transactions {
                        if transaction.is_coinbase() {
                            continue;
                        }
                        let confirmed = McpEvent::TxConfirmed {
                            txid: hex::encode(txid(transaction)),
                            height,
                        };
                        if tx.send(confirmed).await.is_err() {
                            return;
                        }
                    }
                }
                // EventLog needs this block's receipts — available only while
                // it is still the tip (best-effort; see TxRecord note).
                let is_tip = facade.tip().map(|tip| tip.hash == hash).unwrap_or(false);
                if is_tip {
                    for receipt in facade.tip_receipts() {
                        for log in &receipt.logs {
                            let entry = McpEvent::EventLog {
                                height,
                                txid: hex::encode(receipt.txid),
                                address: log.address.clone(),
                                topics: log.topics.iter().map(hex::encode).collect(),
                                data: hex::encode(&log.data),
                            };
                            if tx.send(entry).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        rx
    }
}
