use crate::consensus::verify_transaction;
use crate::error::StrangecoinError;
use crate::{AccountState, Transaction};
use std::collections::{BTreeMap, HashMap};
use strangecoin_core::consensus::{
    MAX_RBF_REPLACEMENTS, RBF_BPS_DENOMINATOR, RBF_MIN_DELTA_BPS,
};
use strangecoin_core::serialize::{serialize_transaction_signed, txid};

pub const MAX_PENDING_TXS: usize = 10_000;

pub type TxId = [u8; 32];

/// Результат [`Mempool::insert`]: обычная вставка или RBF-замена со списком
/// вытесненных txid (для анонса `TxRejected { reason: Replaced }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertOutcome {
    Accepted,
    Replaced(Vec<TxId>),
}

#[derive(Clone)]
pub struct Mempool {
    /// Network this pool accepts transactions for (BUG-S1-004): every
    /// inserted tx must carry this `chain_id`.
    chain_id: u32,
    txs: HashMap<TxId, Transaction>,
    by_sender: HashMap<String, BTreeMap<u64, TxId>>,
    /// Поколение цепочки замен: 0 — исходная tx, N — N-я замена в цепочке.
    generations: HashMap<TxId, u32>,
}

/// Длина канонической (подписанной) сериализации — вес tx для feerate-метрики.
fn signed_len(tx: &Transaction) -> u64 {
    serialize_transaction_signed(tx).len() as u64
}

/// feerate = fee / weight; fee пока = 0, поэтому детерминированный прокси
/// `1 / serialized_len`. Возвращается в микро-единицах (1e-6 tx/байт) —
/// только для сообщений об ошибках и логов.
pub fn feerate_micro(tx: &Transaction) -> u64 {
    1_000_000 / signed_len(tx).max(1)
}

/// Старый feerate × (1 + RBF_MIN_DELTA_BPS/10000) в тех же микро-единицах.
fn required_feerate_micro(old: &Transaction) -> u64 {
    1_000_000 * (RBF_BPS_DENOMINATOR + RBF_MIN_DELTA_BPS)
        / (RBF_BPS_DENOMINATOR * signed_len(old).max(1))
}

/// Правило замены (упрощённый BIP-125): новый feerate ≥ старый × (1 + Δ).
/// feerate_old = 1/old_len, feerate_new = 1/new_len, поэтому сравнение сводится
/// к целочисленному `old_len × DENOM ≥ new_len × (DENOM + Δ)` — без f64, без
/// потери точности на округлениях.
fn feerate_bump_ok(old: &Transaction, new: &Transaction) -> bool {
    signed_len(old) * RBF_BPS_DENOMINATOR
        >= signed_len(new) * (RBF_BPS_DENOMINATOR + RBF_MIN_DELTA_BPS)
}

impl Mempool {
    pub fn new(chain_id: u32) -> Self {
        Mempool {
            chain_id,
            txs: HashMap::new(),
            by_sender: HashMap::new(),
            generations: HashMap::new(),
        }
    }

    /// Конфликтующие tx (та же sender + nonce) и их зависимости — tx той же
    /// sender с большим nonce: они невалидны после замены конфликта и должны
    /// быть вытеснены вместе с ним. Пустой вектор — это не замена.
    pub fn find_replaceable(&self, tx: &Transaction) -> Vec<TxId> {
        let mut out = Vec::new();
        if let Some(map) = self.by_sender.get(&tx.sender) {
            if let Some(&conflict) = map.get(&tx.nonce) {
                out.push(conflict);
                out.extend(map.range(tx.nonce + 1..).map(|(_, id)| *id));
            }
        }
        out
    }

    pub fn insert(
        &mut self,
        tx: Transaction,
        account_state: &AccountState,
    ) -> Result<InsertOutcome, StrangecoinError> {
        let tx_id = txid(&tx);
        if self.txs.contains_key(&tx_id) {
            return Err(StrangecoinError::DuplicateTx);
        }

        verify_transaction(&tx)?;

        if tx.chain_id != self.chain_id {
            return Err(StrangecoinError::InvalidChainId {
                expected: self.chain_id,
                got: tx.chain_id,
            });
        }

        let replaceable = self.find_replaceable(&tx);
        if replaceable.is_empty() {
            if self.txs.len() >= MAX_PENDING_TXS {
                return Err(StrangecoinError::MempoolFull(MAX_PENDING_TXS));
            }
            self.validate_against_pool(&tx, account_state, &[])?;
            self.store(tx_id, tx, 0);
            return Ok(InsertOutcome::Accepted);
        }

        // RBF-путь. Все проверки — до каких-либо мутаций: неудачная замена не
        // должна оставлять полупустой пул (иначе вытеснённые tx потеряны).
        let conflict_id = replaceable[0];
        let conflict_tx = self
            .txs
            .get(&conflict_id)
            .expect("mempool invariant: by_sender entry must be in txs");

        if !feerate_bump_ok(conflict_tx, &tx) {
            return Err(StrangecoinError::RbfFeerateTooLow {
                needed: required_feerate_micro(conflict_tx),
                got: feerate_micro(&tx),
            });
        }

        let generation = self.generations.get(&conflict_id).copied().unwrap_or(0) + 1;
        if generation > MAX_RBF_REPLACEMENTS {
            return Err(StrangecoinError::RbfReplacementLimit(MAX_RBF_REPLACEMENTS));
        }

        // Nonce и баланс считаются уже без вытесняемых tx: они ещё в пуле,
        // поэтому excluded = replaceable.
        self.validate_against_pool(&tx, account_state, &replaceable)?;

        for id in &replaceable {
            self.remove(id);
        }
        self.store(tx_id, tx, generation);

        Ok(InsertOutcome::Replaced(replaceable))
    }

    /// Nonce-цепочка и баланс против пула, не считая `excluded` (вытесняемые
    /// RBF-заменой tx — их расходы и слоты nonce перестают существовать).
    fn validate_against_pool(
        &self,
        tx: &Transaction,
        account_state: &AccountState,
        excluded: &[TxId],
    ) -> Result<(), StrangecoinError> {
        let sender_map = self.by_sender.get(&tx.sender);

        let pending_count = sender_map
            .map(|m| {
                m.values()
                    .filter(|id| !excluded.contains(id))
                    .count() as u64
            })
            .unwrap_or(0);
        // expected = account.nonce + 1 + pending_count; правило — core
        // validate_nonce (BUG-S0-025), маппинг ошибки через From<CoreError>.
        strangecoin_core::consensus::validate_nonce(
            tx.nonce,
            account_state.nonce.saturating_add(pending_count),
        )?;

        let pending_spent: u64 = sender_map
            .map(|m| {
                m.values()
                    .filter(|id| !excluded.contains(id))
                    .filter_map(|id| self.txs.get(id))
                    .map(|t| t.amount)
                    .sum()
            })
            .unwrap_or(0);
        let available = account_state.balance.saturating_sub(pending_spent);
        if tx.amount > available {
            return Err(StrangecoinError::InsufficientBalance {
                available,
                required: tx.amount,
            });
        }

        Ok(())
    }

    fn store(&mut self, tx_id: TxId, tx: Transaction, generation: u32) {
        self.by_sender
            .entry(tx.sender.clone())
            .or_default()
            .insert(tx.nonce, tx_id);
        self.txs.insert(tx_id, tx);
        self.generations.insert(tx_id, generation);
    }

    pub fn remove(&mut self, tx_id: &TxId) {
        self.generations.remove(tx_id);
        if let Some(tx) = self.txs.remove(tx_id) {
            if let Some(sender_map) = self.by_sender.get_mut(&tx.sender) {
                sender_map.remove(&tx.nonce);
                if sender_map.is_empty() {
                    self.by_sender.remove(&tx.sender);
                }
            }
        }
    }

    /// Поколение замены tx (0 — не замена). Для тестов/диагностики.
    pub fn generation(&self, tx_id: &TxId) -> Option<u32> {
        self.generations.get(tx_id).copied()
    }

    pub fn get_pending(&self, max_count: usize) -> Vec<Transaction> {
        self.txs.values().take(max_count).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.txs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    pub fn contains(&self, tx_id: &TxId) -> bool {
        self.txs.contains_key(tx_id)
    }

    pub fn transactions(&self) -> Vec<Transaction> {
        self.txs.values().cloned().collect()
    }
}
