//! # Block executor (ARCHITECT3 §3.4, component 2)
//!
//! «Validate + apply» for a single block, plus block *construction* paths
//! (genesis, grant, mining) that moved out of the facade in S1.5-P04.
//!
//! Checks in `validate_and_apply`:
//!
//! 1. position in the chain (`index`, `previous_hash`);
//! 2. header hash (`block.hash` over the serialized header);
//! 3. `consensus_version`;
//! 4. `tx_root` (merkle commitment to the transaction set);
//! 5. timestamp (MTP + future bound);
//! 6. proof of work and the retarget schedule for `block.target`;
//! 7. transaction signatures (with the opt-in grant-block exemption);
//! 8. state transition: coinbase emission, balances, nonces (`core::state`);
//! 9. `state_root` commitment, when the header carries one.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};

use strangecoin_core::state::{apply_block, compute_state_root, State};
use strangecoin_core::types::{Block, Transaction};
use tracing::{debug, error, info, warn};

use super::blockchain_facade::Blockchain;
use super::consensus_manager::ConsensusPhase;
use crate::error::StrangecoinError;
use crate::GRANT_BLOCK_INDEX;

/// Everything a block is validated against besides its parent state.
pub struct BlockView<'a> {
    pub chain: &'a [Block],
    pub now: u64,
    pub allow_grant_blocks: bool,
    pub expected_consensus_version: u32,
    pub phase: ConsensusPhase,
}

impl<'a> BlockView<'a> {
    pub fn new(
        chain: &'a [Block],
        now: u64,
        allow_grant_blocks: bool,
        expected_consensus_version: u32,
    ) -> Self {
        Self {
            chain,
            now,
            allow_grant_blocks,
            expected_consensus_version,
            phase: ConsensusPhase::Pow,
        }
    }

    pub fn with_phase(mut self, phase: ConsensusPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn next(
        chain: &'a [Block],
        allow_grant_blocks: bool,
        expected_consensus_version: u32,
    ) -> Self {
        Self::new(
            chain,
            now_secs(),
            allow_grant_blocks,
            expected_consensus_version,
        )
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Validate `block` against `parent_state` and `view.chain`, then apply it.
pub fn validate_and_apply(
    parent_state: &State,
    block: &Block,
    view: &BlockView<'_>,
) -> Result<State, StrangecoinError> {
    validate_position(block, view.chain)?;

    let computed_hash = hex::encode(strangecoin_core::serialize::block_hash(block));
    if block.hash != computed_hash {
        return Err(invalid(
            block,
            format!(
                "header hash mismatch: stored {}, computed {}",
                block.hash, computed_hash
            ),
        ));
    }

    if matches!(view.phase, ConsensusPhase::Pos) {
        return Err(invalid(
            block,
            "PoS validation phase is not active until Stage 7",
        ));
    }
    if block.consensus_version != view.expected_consensus_version {
        return Err(invalid(
            block,
            format!(
                "consensus_version mismatch: got {}, expected {}",
                block.consensus_version, view.expected_consensus_version
            ),
        ));
    }

    strangecoin_core::consensus::validate_tx_root(block)?;
    strangecoin_core::consensus::validate_timestamp(block, view.chain, view.now)?;

    if block.index > 0 {
        strangecoin_core::consensus::validate_difficulty(block)?;
        validate_target(block, view.chain)?;
    }

    let is_opt_in_grant_block = view.allow_grant_blocks && block.index == GRANT_BLOCK_INDEX;
    if !is_opt_in_grant_block {
        for tx in &block.transactions {
            strangecoin_core::consensus::verify_transaction(tx)?;
        }
    }

    let new_state = apply_block(parent_state, block)?;

    if block.state_root != [0u8; 32] && block.state_root != compute_state_root(&new_state.balances)
    {
        return Err(invalid(
            block,
            format!(
                "state root mismatch: stored {:x?}, computed {:x?}",
                block.state_root,
                compute_state_root(&new_state.balances)
            ),
        ));
    }

    Ok(new_state)
}

fn validate_position(block: &Block, chain: &[Block]) -> Result<(), StrangecoinError> {
    if block.index == 0 {
        if !chain.is_empty() {
            return Err(invalid(block, "genesis block applied on a non-empty chain"));
        }
        if block.previous_hash != "0".repeat(64) {
            return Err(invalid(block, "genesis previous_hash must be 64 zeroes"));
        }
        return Ok(());
    }

    let parent = chain
        .last()
        .ok_or_else(|| invalid(block, "parent block is missing"))?;
    if block.index as usize != chain.len() {
        return Err(invalid(
            block,
            format!(
                "index {} does not follow parent height {}",
                block.index, parent.index
            ),
        ));
    }
    if block.previous_hash != parent.hash {
        return Err(invalid(
            block,
            format!(
                "previous_hash {} does not match parent hash {}",
                block.previous_hash, parent.hash
            ),
        ));
    }
    Ok(())
}

fn validate_target(block: &Block, chain: &[Block]) -> Result<(), StrangecoinError> {
    if block
        .index
        .is_multiple_of(crate::consensus::RETARGET_INTERVAL)
    {
        let expected = hex::encode(strangecoin_core::consensus::compute_target(chain));
        if block.target != expected {
            return Err(invalid(
                block,
                format!(
                    "target {} at retarget height, expected {}",
                    block.target, expected
                ),
            ));
        }
        return Ok(());
    }

    let parent = chain
        .last()
        .ok_or_else(|| invalid(block, "parent block is missing"))?;
    if block.target != parent.target {
        return Err(invalid(
            block,
            format!(
                "target {} changed outside a retarget height, expected {}",
                block.target, parent.target
            ),
        ));
    }
    Ok(())
}

fn invalid(block: &Block, reason: impl std::fmt::Display) -> StrangecoinError {
    let message = format!("block {}: {}", block.index, reason);
    warn!(block_index = block.index, reason = %reason, "Block rejected");
    StrangecoinError::InvalidBlock(message)
}

// ---------------------------------------------------------------------------
// Blockchain construction / mining (S1.5-P04: moved from blockchain_facade)
// ---------------------------------------------------------------------------

impl Blockchain {
    pub(crate) fn create_genesis_block(&mut self) {
        let genesis_transaction = Transaction {
            sender: "genesis".to_string(),
            receiver: "initial_wallet_address".to_string(),
            amount: 10000,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: true,
        };
        let mut genesis_block = Block {
            index: 0,
            timestamp: 0,
            transactions: vec![genesis_transaction],
            previous_hash: "0".repeat(64),
            hash: String::new(),
            nonce: 0,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            consensus_version: self.rules.expected_version(0),
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        genesis_block.tx_root =
            strangecoin_core::serialize::compute_tx_root(&genesis_block.transactions);
        genesis_block.hash = self.calculate_hash(&genesis_block);

        let applied = {
            let view = BlockView::new(
                &[],
                now_secs(),
                self.allow_grant_blocks,
                self.rules.expected_version(0),
            )
            .with_phase(self.rules.phase_at(0));
            validate_and_apply(&State::new(), &genesis_block, &view)
        };
        match applied {
            Ok(new_state) => {
                self.chain.push(genesis_block);
                self.balances.commit(new_state);
                self.total_work = strangecoin_core::consensus::cumulative_work(&self.chain);
            }
            Err(e) => {
                error!(error = %e, "Не удалось применить генезис-блок");
                return;
            }
        }
        info!("Создание генезис-блока завершено");
    }

    /// Install genesis on an empty chain (fresh DB or wire-deserialize path).
    pub(crate) fn create_grant_block(&mut self, wallet_address: &str, amount: u64) {
        let previous_block = self.chain.last().unwrap().clone();
        let height = previous_block.index + 1;
        let coinbase = Transaction {
            sender: "coinbase".to_string(),
            receiver: wallet_address.to_string(),
            amount: 0,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: true,
        };
        let transfer = Transaction {
            sender: "initial_wallet_address".to_string(),
            receiver: wallet_address.to_string(),
            amount,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: false,
        };
        let mut block = Block {
            index: height,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions: vec![coinbase, transfer],
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target: previous_block.target.clone(),
            consensus_version: self.rules.expected_version(height),
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
        block.hash = self.calculate_hash(&block);

        let applied = {
            let parent_state = self.balances.to_state();
            let view = self.view_for(height);
            validate_and_apply(&parent_state, &block, &view)
        };
        match applied {
            Ok(new_state) => {
                self.chain.push(block);
                self.balances.commit(new_state);
                self.total_work = strangecoin_core::consensus::cumulative_work(&self.chain);
                info!(from = "initial_wallet_address", to = %wallet_address, amount, "Создан блок первичной эмиссии");
            }
            Err(e) => {
                error!(error = %e, "Не удалось применить блок первичной эмиссии к состоянию");
            }
        }
    }

    pub(crate) fn grant_initial_balance_to_first_wallet(
        &mut self,
        wallet_address: &str,
    ) -> Result<bool, StrangecoinError> {
        if !self.allow_grant_blocks {
            warn!("Grant blocks are disabled (allow_grant_blocks = false)");
            return Err(StrangecoinError::GrantBlocksDisabled);
        }
        if self.chain.len() != 1 {
            return Ok(false);
        }
        if self.balances.contains_key(wallet_address) {
            return Ok(false);
        }
        let is_first_wallet =
            self.balances.len() == 1 && self.balances.contains_key("initial_wallet_address");
        if !is_first_wallet {
            return Ok(false);
        }
        let amount = match self.balances.get("initial_wallet_address") {
            Some(a) if a.balance > 0 => a.balance,
            _ => return Ok(false),
        };
        self.create_grant_block(wallet_address, amount);
        info!(wallet = %wallet_address, amount, "Первому кошельку начислен первоначальный баланс");
        Ok(true)
    }

    pub(crate) fn commit_block(&mut self, block: &Block) -> Result<(), StrangecoinError> {
        let new_state = {
            let view = self.view_for(block.index);
            let parent_state = self.balances.to_state();
            validate_and_apply(&parent_state, block, &view)?
        };
        self.balances.commit(new_state);
        for tx in &block.transactions {
            if !tx.is_coinbase {
                self.mempool.remove(&strangecoin_core::serialize::txid(tx));
            }
        }
        self.chain.push(block.clone());
        self.total_work = strangecoin_core::consensus::cumulative_work(&self.chain);
        Ok(())
    }

    pub(crate) fn mine_block(
        &mut self,
        progress_tx: mpsc::Sender<String>,
        shutdown: &Arc<AtomicBool>,
    ) -> Option<Block> {
        info!("Начало майнинга");
        if self.mempool.is_empty() {
            warn!("Нет транзакций для майнинга");
            let _ = progress_tx.send("Нет транзакций для майнинга".to_string());
            return None;
        }

        let previous_block = self.chain.last().unwrap().clone();
        let transactions: Vec<Transaction> = self.mempool.get_pending(1000);
        if transactions.is_empty() {
            warn!("Все pending_transactions уже включены в блоки");
            let _ = progress_tx.send("Все pending_transactions уже включены в блоки".to_string());
            return None;
        }

        let total_supply = self.balances.total_supply();
        let height = previous_block.index + 1;
        let coinbase_amount =
            crate::economics::emission::block_reward_at_height(height, total_supply);

        let miner_address = self
            .balances
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "miner".to_string());
        let coinbase_tx = Transaction {
            sender: "coinbase".to_string(),
            receiver: miner_address,
            amount: coinbase_amount,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: true,
        };

        let mut all_transactions = vec![coinbase_tx];
        all_transactions.extend(transactions);

        let block = self.mine_block_inner(previous_block, all_transactions, progress_tx.clone(), shutdown);

        if let Some(block) = block {
            if let Err(e) = self.commit_block(&block) {
                error!(error = %e, "Failed to apply block to state");
                return None;
            }
            self.save_state();
            info!(block = ?block, "Майнинг завершен, блок добавлен");
            let _ = progress_tx.send("Майнинг завершен".to_string());
            Some(block)
        } else {
            warn!("Майнинг не удался");
            let _ = progress_tx.send("Майнинг не удался".to_string());
            None
        }
    }

    pub(crate) fn mine_block_inner(
        &self,
        previous_block: Block,
        transactions: Vec<Transaction>,
        progress_tx: mpsc::Sender<String>,
        shutdown: &Arc<AtomicBool>,
    ) -> Option<Block> {
        debug!("Начало mine_block_inner");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mtp = crate::consensus::median_time_past(&self.chain, previous_block.index + 1);

        let target =
            if (previous_block.index + 1).is_multiple_of(crate::consensus::RETARGET_INTERVAL) {
                hex::encode(crate::consensus::compute_target(&self.chain))
            } else {
                previous_block.target.clone()
            };

        let height = previous_block.index + 1;
        let mut block = Block {
            index: height,
            timestamp: now.max(mtp + 1),
            transactions,
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target,
            consensus_version: self.rules.expected_version(height),
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);

        let target_bytes = hex::decode(&block.target).expect("valid target hex");
        let mut target_arr = [0u8; 32];
        target_arr.copy_from_slice(&target_bytes);
        let target_u256 = crate::consensus::u256_from_bytes(&target_arr);

        let mut iteration_count = 0u64;
        loop {
            if shutdown.load(Ordering::Relaxed) {
                info!("Shutdown signal received, stopping mining");
                return None;
            }
            iteration_count += 1;
            let hash = self.calculate_hash(&block);

            let hash_bytes = hex::decode(&hash).expect("valid hash hex");
            let mut hash_arr = [0u8; 32];
            hash_arr.copy_from_slice(&hash_bytes);
            let hash_u256 = crate::consensus::u256_from_bytes(&hash_arr);

            if crate::consensus::u256_le(hash_u256, target_u256) {
                block.hash = hash;
                let _ = progress_tx.send(format!(
                    "Подходящий хэш найден после {} итераций",
                    iteration_count
                ));
                return Some(block);
            }
            block.nonce += 1;
            if iteration_count.is_multiple_of(10000) {
                let _ = progress_tx.send(format!("Прогресс майнинга: {} итераций", iteration_count));
            }
        }
    }

    /// Install genesis when the DB has no chain (fresh node).
    pub(crate) fn install_fresh_genesis(blockchain: &mut Blockchain) {
        let chain_id = crate::consensus::current_chain_id();
        let is_regtest = crate::consensus::is_regtest(chain_id);

        let genesis_block = if is_regtest {
            let genesis_tx = Transaction {
                sender: "genesis".to_string(),
                receiver: "regtest_initial_holder".to_string(),
                amount: 1_000_000_000,
                nonce: 0,
                chain_id,
                signature: Vec::new(),
                is_coinbase: true,
            };
            let target_bytes =
                hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
                    .expect("valid target hex");
            let mut target_arr = [0u8; 32];
            target_arr.copy_from_slice(&target_bytes);
            let mut block = Block {
                index: 0,
                timestamp: 0,
                transactions: vec![genesis_tx],
                previous_hash: "0".repeat(64),
                hash: String::new(),
                nonce: 0,
                target: hex::encode(target_arr),
                consensus_version: blockchain.rules.expected_version(0),
                state_root: [0u8; 32],
                tx_root: [0u8; 32],
            };
            block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
            block.hash = hex::encode(strangecoin_core::serialize::block_hash(&block));
            block
        } else {
            let exe_path =
                std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
            let exe_dir = exe_path
                .parent()
                .expect("Не удалось получить директорию исполняемого файла");
            let genesis_path = exe_dir.join("genesis.json");
            crate::consensus::load_genesis(genesis_path.to_str().unwrap())
                .expect("Failed to load genesis from genesis.json")
        };

        if let Err(e) = crate::consensus::validate_genesis(&genesis_block, is_regtest) {
            panic!("Genesis validation failed: {}", e);
        }
        blockchain.chain.push(genesis_block.clone());
        for tx in &genesis_block.transactions {
            if tx.sender != "genesis" {
                blockchain.balances.credit(&tx.receiver, tx.amount);
            }
        }
    }
}
