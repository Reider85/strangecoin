//! [`AcpHandler`] — the ACP inbound validation pipeline (AI-P03,
//! ADR-0030 + `docs/spec/acp_messages.md` §5/§6).
//!
//! Pipeline per message (order is normative — side effects never precede
//! authentication):
//!
//! 1. **Frame decode** — `[tag][canonical JSON]`, tag↔kind consistency,
//!    64 KiB cap ([`AcpReject::Malformed`], protocol violation +1).
//! 2. **Envelope signature** — secp256k1 over
//!    `blake3(canonical_json({message, sender_agent_id}))`
//!    ([`AcpReject::BadSignature`], protocol violation +1).
//! 3. **Party** — `sender ∈ {agent_id_a, agent_id_b}`; counterparty = the
//!    other one ([`AcpReject::NotAParty`], protocol violation +1).
//! 4. **Replay nonce (#25)** — write-ahead into the [`AcpNonceStore`]:
//!    `nonce <= last_seen` → [`AcpReject::Replay`] (protocol violation +1,
//!    spec §9 has no ACP row for replays — they ride the generic protocol
//!    channel); store failure → [`AcpReject::NonceStore`] fail-closed, the
//!    peer is *not* charged for a local fault.
//! 5. **Body validation** — NEGOTIATE records the bilateral quote;
//!    TASK/SETTLE/REVOKE run the escrow checks (invariant #24):
//!    existence → `Call` to the configured `EscrowVault` → parties (tx
//!    sender, calldata addresses, calldata pubkeys) → amount (tx value
//!    covers the recorded amount; ≥ recorded quote when this node has one)
//!    → deadline (task/settle not past deadline; revoke not premature) →
//!    signatures over `blake3(id_bytes)`. Failures map to
//!    [`AcpViolation`] units (spec §5/§9 score table).
//! 6. **Settlement** — accepted SETTLE/REVOKE go to the injected
//!    [`EscrowSettlement`] adapter; an adapter error is a *local*
//!    submission fault ([`AcpReject::Settlement`], no peer-score damage).
//!
//! **Determinism** (Stage 2 checklist applied to ACP): no wall clock, no
//! RNG. The tip height is derived from `chain_len()` (genesis `index = 0`,
//! tip `index = len - 1`); nonces come from the message; quotes live in an
//! in-memory map (session-scoped — ADR-0030 amendment); peer iteration
//! never happens here.

use super::nonce::AcpNonceStore;
use super::{EscrowAction, EscrowSettlement};
use crate::blockchain_view::BlockchainView;
use crate::noise::NodeId;
use crate::peer::scoring::AcpViolation;
use crate::peer::PeerManager;
use crate::protocol::acp::{AcpEnvelope, AcpKind, AcpMessage, AcpQuote, AgentId};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use secp256k1::ecdsa::Signature;
use secp256k1::{Message, PublicKey, Secp256k1};
use strangecoin_core::address::encode_address;
use strangecoin_core::escrow;
use strangecoin_core::types::Transaction;

/// Handler wiring: identity of the *network* (address encoding) plus the
/// deployed EscrowVault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpHandlerConfig {
    /// `chain_id` for bech32 address derivation (`sc1`/`tsc1`/`rsc1`) —
    /// must match the node's network.
    pub network_id: u32,
    /// EscrowVault contract address. `None` disables escrow-referencing
    /// messages (TASK/SETTLE/REVOKE reject locally, not scored — a node
    /// that never deployed the vault cannot judge a peer's escrow).
    pub escrow_contract: Option<String>,
}

/// Why an inbound ACP message was refused — carries the scoring decision:
/// variants marked *scored* charge the peer, the others are local faults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpReject {
    /// Malformed frame/json/kind-mismatch (scored: protocol violation +1).
    Malformed(String),
    /// Envelope signature failed (scored: protocol violation +1).
    BadSignature(String),
    /// `agent_nonce <= last_seen` for `(sender, counterparty)` — invariant
    /// #25 (scored: protocol violation +1).
    Replay {
        /// Last accepted nonce.
        last: u64,
        /// The stale nonce that arrived.
        got: u64,
    },
    /// `sender_agent_id ∉ {agent_id_a, agent_id_b}` — or a quote response
    /// from someone other than agent B (scored: protocol violation +1).
    NotAParty,
    /// Nonce store failure — fail-closed, **not scored** (local).
    NonceStore(String),
    /// `escrow_contract` not configured on this node — **not scored**
    /// (local configuration).
    NotConfigured(String),
    /// Escrow verification failure (invariant #24, V-45) — scored with the
    /// class's [`AcpViolation`] units (spec §5/§9).
    Escrow(AcpViolation),
    /// Local settlement adapter could not submit the on-chain call —
    /// **not scored** (local submission).
    Settlement(String),
    /// Local internal fault (address derivation, …) — **not scored**.
    Local(String),
}

impl fmt::Display for AcpReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcpReject::Malformed(err) => write!(f, "malformed acp frame: {err}"),
            AcpReject::BadSignature(err) => write!(f, "acp envelope signature: {err}"),
            AcpReject::Replay { last, got } => {
                write!(f, "acp replay: nonce {got} <= last_seen {last}")
            }
            AcpReject::NotAParty => write!(f, "acp sender is not a party of this task"),
            AcpReject::NonceStore(err) => write!(f, "acp nonce store: {err}"),
            AcpReject::NotConfigured(err) => write!(f, "acp not configured: {err}"),
            AcpReject::Escrow(v) => write!(f, "acp escrow verification: {v:?}"),
            AcpReject::Settlement(err) => write!(f, "acp settlement submit: {err}"),
            AcpReject::Local(err) => write!(f, "acp local error: {err}"),
        }
    }
}

impl std::error::Error for AcpReject {}

/// A message that passed every check (and triggered settlement, if any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAccepted {
    /// Which of the four messages was accepted.
    pub kind: AcpKind,
    /// Envelope sender (authenticated agent).
    pub sender: AgentId,
    /// Escrow txid for TASK/SETTLE/REVOKE (`None` for NEGOTIATE).
    pub escrow_txid: Option<[u8; 32]>,
    /// Numeric escrow id for TASK/SETTLE/REVOKE (`None` for NEGOTIATE).
    pub escrow_id: Option<u64>,
}

/// Verified escrow facts shared by TASK/SETTLE/REVOKE validation.
struct EscrowFacts {
    /// Decoded deposit calldata (parties, amounts, deadline).
    deposit: escrow::EscrowDepositData,
    /// Raw `blake3(deposit_calldata)[0..8]`.
    id_bytes: [u8; 8],
    /// Numeric id view.
    id: u64,
    /// Tip height at validation time (`chain_len - 1`).
    tip_height: u64,
}

/// ACP validation + peer-scoring sink. Cloneable handles share one set of
/// `Arc`s; the handler itself is stateless besides the quote map (all
/// fields behind short-held locks — safe across connection-loop tasks).
pub struct AcpHandler {
    view: Arc<dyn BlockchainView>,
    nonces: Arc<dyn AcpNonceStore>,
    settlement: Arc<dyn EscrowSettlement>,
    peer_manager: Arc<PeerManager>,
    config: AcpHandlerConfig,
    /// Last quote seen per `(agent_id_a, agent_id_b)` (spec §3.1/§5
    /// «Amount» check). Session-scoped memory — a restart forgets quotes
    /// and the amount-vs-quote check degrades to the unconditional
    /// value/parties checks (ADR-0030 amendment).
    quotes: Mutex<HashMap<(AgentId, AgentId), AcpQuote>>,
}

impl AcpHandler {
    /// Build the handler; every dependency is injected (the net crate never
    /// touches storage/monolith types directly — strangler pattern).
    pub fn new(
        view: Arc<dyn BlockchainView>,
        nonces: Arc<dyn AcpNonceStore>,
        settlement: Arc<dyn EscrowSettlement>,
        peer_manager: Arc<PeerManager>,
        config: AcpHandlerConfig,
    ) -> Self {
        AcpHandler {
            view,
            nonces,
            settlement,
            peer_manager,
            config,
            quotes: Mutex::new(HashMap::new()),
        }
    }

    /// The configured EscrowVault address (`None` = escrow messages will
    /// reject locally).
    pub fn escrow_contract(&self) -> Option<&str> {
        self.config.escrow_contract.as_deref()
    }

    /// Recorded quote for a task pair (observability/tests).
    pub fn quote_for(&self, agent_id_a: AgentId, agent_id_b: AgentId) -> Option<AcpQuote> {
        self.quotes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(agent_id_a, agent_id_b))
            .cloned()
    }

    /// Process one inbound ACP frame (full `[tag][json]` payload as received
    /// by the connection loop). Every refusal charges the peer exactly per
    /// the variant's scoring class — see [`AcpReject`].
    pub fn handle(&self, payload: &[u8], peer: NodeId) -> Result<AcpAccepted, AcpReject> {
        // 1. Frame decode.
        let envelope = AcpEnvelope::decode_wire(payload)
            .map_err(|err| self.scored(peer, AcpReject::Malformed(err.to_string())))?;
        // 2. Envelope signature — authentication before any state effect.
        envelope
            .verify()
            .map_err(|err| self.scored(peer, AcpReject::BadSignature(err.to_string())))?;

        let sender = envelope.sender_agent_id;
        let message = envelope.message;
        let (agent_id_a, agent_id_b) = message.pair();
        let counterparty = if sender == agent_id_a {
            agent_id_b
        } else if sender == agent_id_b {
            agent_id_a
        } else {
            return Err(self.scored(peer, AcpReject::NotAParty));
        };
        // Quote responses are the performer's direction (spec §3.1: `quote`
        // present only B → A) — evaluated with the other party checks,
        // *before* the write-ahead nonce window moves, so every `NotAParty`
        // rejection is pre-auth (the fuzz contract keys off this).
        if let AcpMessage::Negotiate(m) = &message {
            if m.quote.is_some() && sender != m.agent_id_b {
                return Err(self.scored(peer, AcpReject::NotAParty));
            }
        }

        // 4. Write-ahead nonce admission (#25, spec §6.2).
        self.admit_nonce(sender, counterparty, message.nonce())
            .map_err(|reject| self.scored(peer, reject))?;

        // 5. Body validation + (6) settlement.
        match message {
            AcpMessage::Negotiate(m) => {
                if let Some(quote) = m.quote {
                    self.quotes
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert((m.agent_id_a, m.agent_id_b), quote);
                }
                Ok(AcpAccepted {
                    kind: AcpKind::Negotiate,
                    sender,
                    escrow_txid: None,
                    escrow_id: None,
                })
            }
            AcpMessage::Task(m) => {
                let facts =
                    self.verify_escrow(peer, &m.agent_id_a, &m.agent_id_b, &m.escrow_txid)?;
                if facts.tip_height > facts.deposit.deadline_block
                    || m.deadline_block != facts.deposit.deadline_block
                {
                    // ADR-0030 check #4 for the assignment: deadline ahead
                    // and TASK's claim matches the on-chain deposit (a
                    // mismatched deadline would desync revoke rights).
                    return Err(self.scored(peer, AcpReject::Escrow(AcpViolation::EscrowDeadline)));
                }
                Ok(AcpAccepted {
                    kind: AcpKind::Task,
                    sender,
                    escrow_txid: Some(m.escrow_txid),
                    escrow_id: Some(facts.id),
                })
            }
            AcpMessage::Settle(m) => {
                let facts =
                    self.verify_escrow(peer, &m.agent_id_a, &m.agent_id_b, &m.escrow_txid)?;
                if facts.tip_height > facts.deposit.deadline_block {
                    return Err(self.scored(peer, AcpReject::Escrow(AcpViolation::EscrowDeadline)));
                }
                let digest = escrow::escrow_signature_message(&facts.id_bytes);
                if !verify_agent_sig(&m.agent_id_a, &digest, &m.settle_signature_a)
                    || !verify_agent_sig(&m.agent_id_b, &digest, &m.settle_signature_b)
                {
                    return Err(
                        self.scored(peer, AcpReject::Escrow(AcpViolation::ForgedEscrowSignature))
                    );
                }
                let action = EscrowAction::Release {
                    escrow_id: facts.id,
                    id_bytes: facts.id_bytes,
                    escrow_txid: m.escrow_txid,
                    signature_a: m.settle_signature_a,
                    signature_b: m.settle_signature_b,
                };
                self.settlement
                    .release(&action)
                    .map_err(|err| self.scored(peer, AcpReject::Settlement(err)))?;
                Ok(AcpAccepted {
                    kind: AcpKind::Settle,
                    sender,
                    escrow_txid: Some(m.escrow_txid),
                    escrow_id: Some(facts.id),
                })
            }
            AcpMessage::Revoke(m) => {
                let facts =
                    self.verify_escrow(peer, &m.agent_id_a, &m.agent_id_b, &m.escrow_txid)?;
                let digest = escrow::escrow_signature_message(&facts.id_bytes);
                let action = if m.deadline_passed {
                    // Unilateral timeout (spec §3.4): the deposit deadline
                    // must have passed (contract re-checks it in the
                    // timeout-form refund); only sig_a is required.
                    if facts.tip_height <= facts.deposit.deadline_block {
                        return Err(
                            self.scored(peer, AcpReject::Escrow(AcpViolation::EscrowDeadline))
                        );
                    }
                    if !verify_agent_sig(&m.agent_id_a, &digest, &m.revoke_signature_a) {
                        return Err(self
                            .scored(peer, AcpReject::Escrow(AcpViolation::ForgedEscrowSignature)));
                    }
                    (m.revoke_signature_b, true)
                } else {
                    // Mutual cancel: both signatures, no height constraint.
                    let Some(signature_b) = m.revoke_signature_b else {
                        return Err(self.scored(
                            peer,
                            AcpReject::Malformed(
                                "mutual revoke requires revoke_signature_b".into(),
                            ),
                        ));
                    };
                    if !verify_agent_sig(&m.agent_id_a, &digest, &m.revoke_signature_a)
                        || !verify_agent_sig(&m.agent_id_b, &digest, &signature_b)
                    {
                        return Err(self
                            .scored(peer, AcpReject::Escrow(AcpViolation::ForgedEscrowSignature)));
                    }
                    (Some(signature_b), false)
                };
                let (signature_b, timeout_mode) = action;
                let action = EscrowAction::Refund {
                    escrow_id: facts.id,
                    id_bytes: facts.id_bytes,
                    escrow_txid: m.escrow_txid,
                    signature_a: m.revoke_signature_a,
                    // Timeout mode drops any stray sig_b so the contract
                    // takes the deadline-checked 76-byte form.
                    signature_b: if timeout_mode { None } else { signature_b },
                };
                self.settlement
                    .refund(&action)
                    .map_err(|err| self.scored(peer, AcpReject::Settlement(err)))?;
                Ok(AcpAccepted {
                    kind: AcpKind::Revoke,
                    sender,
                    escrow_txid: Some(m.escrow_txid),
                    escrow_id: Some(facts.id),
                })
            }
        }
    }

    /// Charge the peer according to the reject class, then hand the reject
    /// back for `Err` propagation. Local faults (`NonceStore`,
    /// `NotConfigured`, `Settlement`, `Local`) never touch the score.
    fn scored(&self, peer: NodeId, reject: AcpReject) -> AcpReject {
        match &reject {
            AcpReject::Malformed(_)
            | AcpReject::BadSignature(_)
            | AcpReject::Replay { .. }
            | AcpReject::NotAParty => {
                self.peer_manager.record_protocol_violation(peer);
            }
            AcpReject::Escrow(violation) => {
                self.peer_manager.record_acp_violation(peer, *violation);
            }
            AcpReject::NonceStore(_)
            | AcpReject::NotConfigured(_)
            | AcpReject::Settlement(_)
            | AcpReject::Local(_) => {}
        }
        reject
    }

    /// Replay check + write-ahead store (invariant #25).
    fn admit_nonce(
        &self,
        sender: AgentId,
        counterparty: AgentId,
        nonce: u64,
    ) -> Result<(), AcpReject> {
        let last = self
            .nonces
            .last_nonce(sender, counterparty)
            .map_err(AcpReject::NonceStore)?;
        if let Some(previous) = last {
            if nonce <= previous {
                return Err(AcpReject::Replay {
                    last: previous,
                    got: nonce,
                });
            }
        }
        self.nonces
            .store_nonce(sender, counterparty, nonce)
            .map_err(AcpReject::NonceStore)?;
        Ok(())
    }

    /// Escrow verification for TASK/SETTLE/REVOKE (invariant #24) — every
    /// refusal is already scored through [`scored`](Self::scored).
    fn verify_escrow(
        &self,
        peer: NodeId,
        agent_id_a: &AgentId,
        agent_id_b: &AgentId,
        txid: &[u8; 32],
    ) -> Result<EscrowFacts, AcpReject> {
        let violation = |v: AcpViolation| self.scored(peer, AcpReject::Escrow(v));

        let Some(contract) = self.config.escrow_contract.as_deref() else {
            return Err(self.scored(
                peer,
                AcpReject::NotConfigured(
                    "escrow_contract not configured — escrow messages rejected locally".into(),
                ),
            ));
        };

        // Existence: confirmed in the canonical chain (transactions CF).
        let Some(tx) = self.view.get_tx(txid) else {
            return Err(violation(AcpViolation::MissingEscrowTx));
        };
        // Type: a Call to the configured vault.
        let Transaction::Call(call) = tx else {
            return Err(violation(AcpViolation::WrongEscrowType));
        };
        if call.to != contract {
            return Err(violation(AcpViolation::WrongEscrowType));
        }
        let deposit = escrow::decode_deposit(&call.data)
            .map_err(|_| violation(AcpViolation::WrongEscrowType))?;

        // Parties: bech32 addresses derived from the ACP agent ids must
        // match both the tx sender and the calldata — and the calldata
        // pubkeys must *be* the ACP agent ids (sig-binding).
        let pk_a = PublicKey::from_slice(&agent_id_a.0)
            .map_err(|_| violation(AcpViolation::EscrowPartyMismatch))?;
        let pk_b = PublicKey::from_slice(&agent_id_b.0)
            .map_err(|_| violation(AcpViolation::EscrowPartyMismatch))?;
        let addr_a = encode_address(&pk_a, self.config.network_id)
            .map_err(|err| self.scored(peer, AcpReject::Local(format!("address a: {err}"))))?;
        let addr_b = encode_address(&pk_b, self.config.network_id)
            .map_err(|err| self.scored(peer, AcpReject::Local(format!("address b: {err}"))))?;
        if call.from != addr_a || deposit.agent_a != addr_a || deposit.agent_b != addr_b {
            return Err(violation(AcpViolation::EscrowPartyMismatch));
        }
        if deposit.pk_a != agent_id_a.0 || deposit.pk_b != agent_id_b.0 {
            return Err(violation(AcpViolation::EscrowPartyMismatch));
        }

        // Amount: tx value must cover the recorded amount (the contract
        // re-binds amount := tx value through `sc.value`); vs the recorded
        // quote when this node has one (spec §5 «Amount» row — conditional,
        // ADR-0030 amendment: the counterparty's node never saw the
        // B → A quote response).
        if call.value < deposit.amount {
            return Err(violation(AcpViolation::EscrowAmountMismatch));
        }
        if let Some(quote) = self.quote_for(*agent_id_a, *agent_id_b) {
            if deposit.amount < quote.amount_sc {
                return Err(violation(AcpViolation::EscrowAmountMismatch));
            }
        }

        // Genesis index = 0 ⇒ tip height = chain_len - 1 (regtest genesis
        // and every child follow `index = previous + 1`).
        let tip_height = self.view.chain_len().saturating_sub(1) as u64;
        let id_bytes = escrow::escrow_id_bytes(&call.data);
        let id = u64::from_le_bytes(id_bytes);
        Ok(EscrowFacts {
            deposit,
            id_bytes,
            id,
            tip_height,
        })
    }
}

/// secp256k1 verify of an agent's signature over the 32-byte escrow digest
/// (`secp256k1_verify` host semantics: the digest is the message directly).
fn verify_agent_sig(agent: &AgentId, digest: &[u8; 32], signature: &[u8; 64]) -> bool {
    let Ok(pk) = PublicKey::from_slice(&agent.0) else {
        return false;
    };
    let Ok(sig) = Signature::from_compact(signature) else {
        return false;
    };
    let msg = Message::from_digest(*digest);
    Secp256k1::verification_only()
        .verify_ecdsa(&msg, &sig, &pk)
        .is_ok()
}
