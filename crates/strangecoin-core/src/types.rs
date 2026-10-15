use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::consensus::U256;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Block {
    pub index: u64,
    pub timestamp: u64,
    pub transactions: Vec<Transaction>,
    pub previous_hash: String,
    pub hash: String,
    pub nonce: u64,
    pub target: String,
    #[serde(default)]
    pub consensus_version: u32,
    #[serde(default)]
    pub state_root: [u8; 32],
    #[serde(default)]
    pub tx_root: [u8; 32],
}

/// The header half of a [`Block`]: everything the canonical header encoding
/// (and therefore the PoW hash) commits to, minus the transaction bodies.
///
/// `hash` is derived — the wire encoding excludes it and the decoder
/// recomputes it — so a peer cannot lie about a header's identity.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockHeader {
    pub index: u64,
    pub timestamp: u64,
    pub previous_hash: String,
    pub hash: String,
    pub nonce: u64,
    pub target: String,
    pub consensus_version: u32,
    pub state_root: [u8; 32],
    pub tx_root: [u8; 32],
}

impl Block {
    /// Header view of this block (fields only, transactions dropped).
    pub fn header(&self) -> BlockHeader {
        BlockHeader {
            index: self.index,
            timestamp: self.timestamp,
            previous_hash: self.previous_hash.clone(),
            hash: self.hash.clone(),
            nonce: self.nonce,
            target: self.target.clone(),
            consensus_version: self.consensus_version,
            state_root: self.state_root,
            tx_root: self.tx_root,
        }
    }

    /// Recover a full block from a header plus its transaction bodies.
    pub fn from_header(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Block {
            index: header.index,
            timestamp: header.timestamp,
            transactions,
            previous_hash: header.previous_hash,
            hash: header.hash,
            nonce: header.nonce,
            target: header.target,
            consensus_version: header.consensus_version,
            state_root: header.state_root,
            tx_root: header.tx_root,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EoaAccount {
    pub balance: u64,
    pub nonce: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContractAccount {
    pub code_hash: [u8; 32],
    pub storage_root: [u8; 32],
    pub nonce: u64,
    pub balance: u64,
}

/// Account model (S1.5-P07): an EOA is controlled by a secp256k1 keypair and
/// carries only `balance`/`nonce`; a contract account additionally commits to
/// its code (`code_hash`) and the root of its storage subtree
/// (`storage_root` — an SMT root scoped to the contract, not the global
/// block state root).
///
/// Serde stays compatible with the Stage 0/1 flat `{balance, nonce}` JSON:
/// an EOA serializes to that exact shape; a contract adds `code_hash` and
/// `storage_root` keys. Deserialization accepts the flat form (→ Eoa) and
/// the contract form (any of the two extra keys present).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Account {
    Eoa(EoaAccount),
    Contract(ContractAccount),
}

/// Backward-compatible name for the pre-Stage-1.5 two-field account. Type
/// name usages keep compiling; struct literals and field accesses migrate to
/// [`Account`] constructors and accessors.
pub type AccountState = Account;

#[derive(Serialize, Deserialize)]
struct AccountFlat {
    #[serde(default)]
    balance: u64,
    #[serde(default)]
    nonce: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code_hash: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    storage_root: Option<[u8; 32]>,
}

impl Serialize for Account {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Account::Eoa(eoa) => AccountFlat {
                balance: eoa.balance,
                nonce: eoa.nonce,
                code_hash: None,
                storage_root: None,
            }
            .serialize(serializer),
            Account::Contract(contract) => AccountFlat {
                balance: contract.balance,
                nonce: contract.nonce,
                code_hash: Some(contract.code_hash),
                storage_root: Some(contract.storage_root),
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Account {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let flat = AccountFlat::deserialize(deserializer)?;
        match (flat.code_hash, flat.storage_root) {
            (None, None) => Ok(Account::Eoa(EoaAccount {
                balance: flat.balance,
                nonce: flat.nonce,
            })),
            (Some(code_hash), storage_root) => Ok(Account::Contract(ContractAccount {
                code_hash,
                storage_root: storage_root.unwrap_or([0u8; 32]),
                nonce: flat.nonce,
                balance: flat.balance,
            })),
            (None, Some(_)) => Err(serde::de::Error::custom(
                "storage_root without code_hash is not a valid account",
            )),
        }
    }
}

impl Default for Account {
    fn default() -> Self {
        Account::Eoa(EoaAccount {
            balance: 0,
            nonce: 0,
        })
    }
}

impl Account {
    pub fn eoa(balance: u64, nonce: u64) -> Self {
        Account::Eoa(EoaAccount { balance, nonce })
    }

    pub fn contract(code_hash: [u8; 32], storage_root: [u8; 32]) -> Self {
        Account::Contract(ContractAccount {
            code_hash,
            storage_root,
            nonce: 0,
            balance: 0,
        })
    }

    pub fn balance(&self) -> u64 {
        match self {
            Account::Eoa(account) => account.balance,
            Account::Contract(account) => account.balance,
        }
    }

    pub fn nonce(&self) -> u64 {
        match self {
            Account::Eoa(account) => account.nonce,
            Account::Contract(account) => account.nonce,
        }
    }

    pub fn set_balance(&mut self, balance: u64) {
        match self {
            Account::Eoa(account) => account.balance = balance,
            Account::Contract(account) => account.balance = balance,
        }
    }

    pub fn set_nonce(&mut self, nonce: u64) {
        match self {
            Account::Eoa(account) => account.nonce = nonce,
            Account::Contract(account) => account.nonce = nonce,
        }
    }

    pub fn is_contract(&self) -> bool {
        matches!(self, Account::Contract(_))
    }

    pub fn as_contract(&self) -> Option<&ContractAccount> {
        match self {
            Account::Contract(account) => Some(account),
            Account::Eoa(_) => None,
        }
    }

    /// An account carries no information iff it is absent from the state: an
    /// EOA with neither balance nor nonce, or a phantom contract record with
    /// an all-zero shape (a deployed contract always has a nonzero
    /// `code_hash`). Contract accounts are never pruned on zero balance.
    pub fn is_empty(&self) -> bool {
        match self {
            Account::Eoa(account) => account.balance == 0 && account.nonce == 0,
            Account::Contract(account) => {
                account.balance == 0
                    && account.nonce == 0
                    && account.code_hash == [0u8; 32]
                    && account.storage_root == [0u8; 32]
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Transaction {
    pub sender: String,
    pub receiver: String,
    pub amount: u64,
    #[serde(default)]
    pub nonce: u64,
    #[serde(default)]
    pub chain_id: u32,
    #[serde(default)]
    pub signature: Vec<u8>,
    #[serde(default)]
    pub is_coinbase: bool,
}

/// Chain snapshot for sync channel and GUI updates
#[derive(Clone, Debug)]
pub struct ChainSnapshot {
    pub chain: Vec<Block>,
    pub balances: HashMap<String, AccountState>,
    pub difficulty: u32,
    pub mempool_txs: Vec<Transaction>,
    pub total_work: U256,
}
