//! # Block executor (ARCHITECT3 §3.4, component 2)
//!
//! «Validate + apply» for a single block, plus block *construction* paths
//! (genesis, grant, mining) that moved out of the facade in S1.5-P04.
//!
//! Checks in `validate_and_apply`:
//!
//! 0. size limits (invariant #7, P12 / BUG-S1-006): the serialized block and
//!    every signed transaction stay under `MAX_BLOCK_SIZE` / `MAX_TX_SIZE` —
//!    the wire-level limits now also hold for internally deserialized chains;
//! 1. position in the chain (`index`, `previous_hash`);
//! 2. header hash (`block.hash` over the serialized header);
//! 3. `consensus_version`;
//! 4. `tx_root` (merkle commitment to the transaction set);
//! 5. timestamp (MTP + future bound);
//! 6. proof of work and the retarget schedule for `block.target`;
//! 7. transaction signatures (with the opt-in grant-block exemption);
//! 8. state transition: coinbase emission, balances, nonces (`core::state`);
//! 9. `state_root` commitment — mandatory after genesis (invariant #19,
//!    SCIP-0002); zero roots only under the regtest opt-in flag;
//! 10. block gas limit (invariant #15, first enforced at Stage 1.5,
//!     ADR-0014): `sum(CallTx.gas_limit) <= BLOCK_GAS_LIMIT` before the
//!     state transition, plus the post-apply `sum(receipt.gas_used)` bound
//!     (S1.5-P15).

use std::collections::HashMap;
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
    /// Legacy regtest opt-in (SCIP-0002 / BUG-S1-002): accept blocks whose
    /// header carries a zero `state_root` (no commitment). Default `false` —
    /// mainnet/testnet always require a state commitment after genesis.
    pub allow_zero_state_root: bool,
    pub expected_consensus_version: u32,
    pub phase: ConsensusPhase,
    /// Network the block is validated for (BUG-S1-004): every tx must carry
    /// this `chain_id`, and the coinbase reward schedule follows it.
    pub chain_id: u32,
    /// Contract code registry available to `Create`/`Call` execution
    /// (S1.5-P14): address → WASM bytecode, reconstructed from the chain.
    pub codes: Option<&'a HashMap<String, Vec<u8>>>,
}

impl<'a> BlockView<'a> {
    pub fn new(
        chain: &'a [Block],
        now: u64,
        allow_grant_blocks: bool,
        expected_consensus_version: u32,
        chain_id: u32,
    ) -> Self {
        Self {
            chain,
            now,
            allow_grant_blocks,
            allow_zero_state_root: false,
            expected_consensus_version,
            phase: ConsensusPhase::Pow,
            chain_id,
            codes: None,
        }
    }

    pub fn with_phase(mut self, phase: ConsensusPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn with_allow_zero_state_root(mut self, allow: bool) -> Self {
        self.allow_zero_state_root = allow;
        self
    }

    pub fn with_codes(mut self, codes: &'a HashMap<String, Vec<u8>>) -> Self {
        self.codes = Some(codes);
        self
    }

    pub fn next(
        chain: &'a [Block],
        allow_grant_blocks: bool,
        expected_consensus_version: u32,
        chain_id: u32,
    ) -> Self {
        Self::new(
            chain,
            now_secs(),
            allow_grant_blocks,
            expected_consensus_version,
            chain_id,
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
    // Invariant #7 / P12 (BUG-S1-006): bound block and per-transaction sizes
    // before any other validation. Wire parsing already enforces these limits,
    // but a `ChainSnapshot` deserialized and adopted through `adopt_candidate`
    // must not bypass them at the validation layer.
    let block_size = strangecoin_core::serialize::serialize_block(block).len();
    if block_size > crate::network::protocol::MAX_BLOCK_SIZE {
        warn!(
            block_index = block.index,
            block_size,
            limit = crate::network::protocol::MAX_BLOCK_SIZE,
            "Block size limit exceeded"
        );
        return Err(StrangecoinError::SizeLimitExceeded("block"));
    }
    for tx in &block.transactions {
        let tx_size = strangecoin_core::serialize::serialize_transaction_signed(tx).len();
        if tx_size > crate::network::protocol::MAX_TX_SIZE {
            warn!(
                block_index = block.index,
                tx_size,
                limit = crate::network::protocol::MAX_TX_SIZE,
                "Transaction size limit exceeded"
            );
            return Err(StrangecoinError::SizeLimitExceeded("transaction"));
        }
    }

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
            // Invariant #10 / BUG-S1-004: a block may only carry transactions
            // of the network this node validates. The state machine repeats
            // the check; failing early here gives a block-scoped error.
            if tx.chain_id() != view.chain_id {
                return Err(StrangecoinError::InvalidChainId {
                    expected: view.chain_id,
                    got: tx.chain_id(),
                });
            }
            strangecoin_core::consensus::verify_transaction(tx)?;
        }
    }

    // Invariant #15 / ADR-0014 (S1.5-P09): the block's total gas reservation
    // may not exceed the fixed block gas limit. Reserved gas is summed from
    // `Call` transactions (full `gas_limit` is held up-front); transfers and
    // creates reserve nothing.
    enforce_block_gas_limit(&block.transactions)?;

    let new_state = apply_and_execute(parent_state, block, view)?;

    // Invariant #15 (S1.5-P15): post-execution metering — the gas actually
    // burned by the block's receipts may not exceed the limit either.
    // Per-tx `gas_used <= gas_limit` makes this implied by the reservation
    // check above; it is enforced explicitly so a metering bug cannot
    // silently overshoot.
    enforce_receipt_gas_limit(&new_state.receipts)?;

    // Invariant #19 / SCIP-0002 (BUG-S1-002): every non-genesis block must
    // commit to its post-state root. A zero root is tolerated only for
    // genesis or under the explicit regtest `allow_zero_state_root` opt-in.
    let computed_root = compute_state_root(&new_state.balances);
    if block.state_root == [0u8; 32] {
        let zero_allowed = block.index == 0 || view.allow_zero_state_root;
        if !zero_allowed {
            return Err(invalid(
                block,
                "state root is zero but a state commitment is required",
            ));
        }
    } else if block.state_root != computed_root {
        return Err(invalid(
            block,
            format!(
                "state root mismatch: stored {:x?}, computed {:x?}",
                block.state_root, computed_root
            ),
        ));
    }

    Ok(new_state)
}

/// Invariant #15 (ADR-0014): reject blocks whose summed `Call` gas
/// reservations exceed [`strangecoin_core::vm::BLOCK_GAS_LIMIT`].
///
/// `Transfer` and `Create` transactions carry no `gas_limit` and reserve
/// nothing; actual execution metering is checked post-apply by
/// [`enforce_receipt_gas_limit`] (S1.5-P15).
pub(crate) fn enforce_block_gas_limit(
    transactions: &[Transaction],
) -> Result<(), StrangecoinError> {
    let total: u64 = transactions
        .iter()
        .map(|tx| match tx {
            Transaction::Call(call) => call.gas_limit,
            Transaction::Transfer(_) | Transaction::Create(_) => 0,
        })
        .sum();
    if total > strangecoin_core::vm::BLOCK_GAS_LIMIT {
        return Err(StrangecoinError::BlockGasLimitExceeded {
            total,
            limit: strangecoin_core::vm::BLOCK_GAS_LIMIT,
        });
    }
    Ok(())
}

/// Invariant #15 (S1.5-P15): reject blocks whose receipts burned more gas
/// in aggregate than [`strangecoin_core::vm::BLOCK_GAS_LIMIT`].
pub(crate) fn enforce_receipt_gas_limit(
    receipts: &[strangecoin_core::vm::Receipt],
) -> Result<(), StrangecoinError> {
    let total: u64 = receipts.iter().map(|receipt| receipt.gas_used).sum();
    if total > strangecoin_core::vm::BLOCK_GAS_LIMIT {
        return Err(StrangecoinError::BlockGasLimitExceeded {
            total,
            limit: strangecoin_core::vm::BLOCK_GAS_LIMIT,
        });
    }
    Ok(())
}

/// State transition of `block`: with the `vm-wasm` feature, `Create`/`Call`
/// transactions execute through a wasmi executor seeded from `view.codes`
/// (S1.5-P14) and every transaction produces a receipt; without the feature
/// (light node) the transfer-only path rejects contract transactions.
fn apply_and_execute(
    parent_state: &State,
    block: &Block,
    view: &BlockView<'_>,
) -> Result<State, StrangecoinError> {
    #[cfg(feature = "vm-wasm")]
    {
        let mut executor = strangecoin_vm_wasm::WasmiExecutor::new();
        if let Some(codes) = view.codes {
            for (address, wasm) in codes {
                executor.insert_code(address, wasm.clone());
            }
        }
        Ok(strangecoin_core::state::apply_block_with_vm(
            parent_state,
            block,
            view.chain_id,
            &mut executor,
        )?)
    }
    #[cfg(not(feature = "vm-wasm"))]
    {
        let _ = view;
        Ok(apply_block(parent_state, block, view.chain_id)?)
    }
}

/// Reconstruct the contract code registry from a chain's `Create`
/// transactions (S1.5-P14, invariant #1 style): deterministic, wire-independent.
pub(crate) fn codes_from_chain(
    chain: &[Block],
    chain_id: u32,
) -> HashMap<String, Vec<u8>> {
    let mut codes = HashMap::new();
    for block in chain {
        collect_block_codes(block, chain_id, &mut codes);
    }
    codes
}

/// Merge the `Create` codes of one block into `codes`.
pub(crate) fn collect_block_codes(
    block: &Block,
    chain_id: u32,
    codes: &mut HashMap<String, Vec<u8>>,
) {
    for tx in &block.transactions {
        if let Transaction::Create(create) = tx {
            if let Ok(address) =
                strangecoin_core::address::derive_contract_address(&create.from, create.nonce, chain_id)
            {
                codes.insert(address, create.init_code.clone());
            }
        }
    }
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
        let genesis_transaction = Transaction::coinbase(
            "genesis",
            "initial_wallet_address",
            10000,
            self.chain_id,
        );
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
                self.chain_id,
            )
            .with_phase(self.rules.phase_at(0));
            validate_and_apply(&State::new(), &genesis_block, &view)
        };
        match applied {
            Ok(new_state) => {
                self.chain.push(genesis_block);
                self.receipts = new_state.receipts.clone();
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
        let coinbase =
            Transaction::coinbase("coinbase", wallet_address, 0, self.chain_id);
        let transfer = Transaction::transfer(
            "initial_wallet_address",
            wallet_address,
            amount,
            0,
            self.chain_id,
        );
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

        // BUG-S1-002 / SCIP-0002: grant blocks commit to their post-state
        // root like any other block.
        let parent_state = self.balances.to_state();
        let post = apply_block(&parent_state, &block, self.chain_id).map_err(|e| {
            error!(error = %e, "Grant block transactions do not apply to the state");
        });
        match post {
            Ok(post) => block.state_root = compute_state_root(&post.balances),
            Err(_) => return,
        }
        block.hash = self.calculate_hash(&block);

        let applied = {
            let view = self.view_for(height);
            validate_and_apply(&parent_state, &block, &view)
        };
        match applied {
            Ok(new_state) => {
                self.chain.push(block);
                self.receipts = new_state.receipts.clone();
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
            Some(a) if a.balance() > 0 => a.balance(),
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
        self.receipts = new_state.receipts.clone();
        self.balances.commit(new_state);
        collect_block_codes(block, self.chain_id, &mut self.codes);
        for tx in &block.transactions {
            if !tx.is_coinbase() {
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
        let coinbase_amount = crate::economics::emission::block_reward_at_height_for_chain(
            height,
            total_supply,
            self.chain_id,
        );

        let miner_address = self
            .balances
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "miner".to_string());
        let coinbase_tx = Transaction::coinbase("coinbase", &miner_address, coinbase_amount, self.chain_id);

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

        // BUG-S1-002 / SCIP-0002: mined blocks commit to their post-state
        // root; a zero root would now be rejected by validate_and_apply.
        // S1.5-P17: mine through the VM-aware executor — the transfer-only
        // path rejected mempool Create/Call transactions with
        // UnsupportedTxType, making contract transactions unminable.
        let parent_state = self.balances.to_state();
        let post = {
            let view = self.view_for(height);
            match apply_and_execute(&parent_state, &block, &view) {
                Ok(state) => state,
                Err(e) => {
                    error!(error = %e, "Мемпул-транзакции не применимы к состоянию");
                    return None;
                }
            }
        };
        block.state_root = compute_state_root(&post.balances);

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
    ///
    /// Regtest builds its own in-code genesis; mainnet/testnet load the
    /// network-specific genesis file and validate it against the committed
    /// expected hash (BUG-S1-004: the network comes from `Config.network_id`
    /// via `Blockchain::chain_id`).
    pub(crate) fn install_fresh_genesis(blockchain: &mut Blockchain) {
        let chain_id = blockchain.chain_id;

        let genesis_block = if crate::consensus::is_regtest(chain_id) {
            crate::consensus::regtest_genesis_block(blockchain.rules.expected_version(0))
        } else {
            let genesis_path = crate::consensus::genesis_file_for_chain(chain_id)
                .unwrap_or_else(|| {
                    panic!("No genesis file configured for network_id {}", chain_id)
                });
            let block = crate::consensus::load_genesis(genesis_path.to_str().unwrap())
                .expect("Failed to load genesis from genesis file");
            let declared = block
                .transactions
                .first()
                .map(|tx| tx.chain_id())
                .unwrap_or(0);
            if declared != chain_id {
                panic!(
                    "Genesis network mismatch: {} declares chain_id {}, node runs {}",
                    genesis_path.display(),
                    declared,
                    chain_id
                );
            }
            block
        };

        if let Err(e) = crate::consensus::validate_genesis(&genesis_block, chain_id) {
            panic!("Genesis validation failed: {}", e);
        }
        blockchain.chain.push(genesis_block.clone());
        for tx in &genesis_block.transactions {
            if tx.sender() != "genesis" {
                if let Some(receiver) = tx.receiver() {
                    blockchain.balances.credit(receiver, tx.value());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use strangecoin_core::types::Transaction;
    use strangecoin_core::vm::BLOCK_GAS_LIMIT;

    use super::enforce_block_gas_limit;
    use crate::error::StrangecoinError;

    fn call_tx(gas_limit: u64) -> Transaction {
        Transaction::call("sc1from", "sc1to", Vec::new(), 0, gas_limit, 0, 3)
    }

    #[test]
    fn empty_block_has_no_gas_usage() {
        assert!(enforce_block_gas_limit(&[]).is_ok());
    }

    #[test]
    fn transfers_and_creates_reserve_no_gas() {
        let txs = vec![
            Transaction::transfer("sc1a", "sc1b", 10, 0, 3),
            Transaction::create("sc1a", vec![0x00, 0x61, 0x73, 0x6d], Vec::new(), 0, 1, 3),
        ];
        assert!(enforce_block_gas_limit(&txs).is_ok());
    }

    #[test]
    fn block_at_gas_limit_is_accepted() {
        let txs = vec![
            call_tx(BLOCK_GAS_LIMIT / 2),
            call_tx(BLOCK_GAS_LIMIT / 2),
        ];
        assert!(enforce_block_gas_limit(&txs).is_ok());
    }

    #[test]
    fn block_over_gas_limit_is_rejected() {
        let txs = vec![
            call_tx(BLOCK_GAS_LIMIT / 2),
            call_tx(BLOCK_GAS_LIMIT / 2 + 1),
        ];
        let err = enforce_block_gas_limit(&txs).expect_err("sum exceeds limit");
        assert!(matches!(
            err,
            StrangecoinError::BlockGasLimitExceeded { total, limit }
                if total == BLOCK_GAS_LIMIT + 1 && limit == BLOCK_GAS_LIMIT
        ));
    }

    #[test]
    fn single_tx_over_gas_limit_is_rejected() {
        let err =
            enforce_block_gas_limit(&[call_tx(BLOCK_GAS_LIMIT + 1)]).expect_err("over limit");
        assert!(matches!(err, StrangecoinError::BlockGasLimitExceeded { .. }));
    }

    fn receipt(gas_used: u64) -> strangecoin_core::vm::Receipt {
        strangecoin_core::vm::Receipt {
            txid: [0u8; 32],
            gas_used,
            logs: Vec::new(),
            status: strangecoin_core::vm::TxStatus::Success,
            block_height: 1,
            tx_index: 0,
        }
    }

    #[test]
    fn receipt_gas_at_limit_is_accepted() {
        let receipts = vec![receipt(BLOCK_GAS_LIMIT / 2), receipt(BLOCK_GAS_LIMIT / 2)];
        assert!(super::enforce_receipt_gas_limit(&receipts).is_ok());
    }

    #[test]
    fn receipt_gas_over_limit_is_rejected() {
        let receipts = vec![receipt(BLOCK_GAS_LIMIT / 2), receipt(BLOCK_GAS_LIMIT / 2 + 1)];
        let err = super::enforce_receipt_gas_limit(&receipts).expect_err("sum exceeds limit");
        assert!(matches!(
            err,
            StrangecoinError::BlockGasLimitExceeded { total, limit }
                if total == BLOCK_GAS_LIMIT + 1 && limit == BLOCK_GAS_LIMIT
        ));
    }
}
