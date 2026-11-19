//! AI-P03 (ADR-0030) — ACP integration suite: the five КГ tests
//! (`acp_negotiate_task_settle_e2e`, `acp_revoke_after_timeout`,
//! `acp_escrow_fraud_rejected`, `acp_replay_rejected`,
//! `acp_spam_rate_limited`) plus the three spec §11 extras
//! (`acp_revoke_mutual_cancel`, `acp_premature_revoke_rejected`,
//! `acp_forged_settle_signature_rejected`).
//!
//! Chain fixtures run through the real facade: grant → deploy `EscrowVault`
//! (WAT from the vm-wasm example, pulled in via `#[path]` so the contract
//! source has exactly one home) → deposit → mined blocks; the handler
//! validates ACP frames exactly as the connection loop feeds them, and the
//! test settlement adapter submits the real `EscrowVault.release/refund`
//! calls (the production `NoopEscrowVault` waits for AI-P04 session keys).
//!
//! Determinism: fixed key seeds, injected rate-limiter clock (`t0`), no
//! sleeps — heights move through crafted blocks (`craft_child`), never wall
//! time.

#![cfg(all(feature = "net", feature = "vm-wasm"))]

mod common;

use std::sync::Arc;
use std::time::Instant;

use common::*;
use strangecoin::BlockchainFacade;
use strangecoin_core::address::{derive_contract_address, encode_address};
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::escrow::{self, EscrowDepositParams};
use strangecoin_core::serialize::txid;
use strangecoin_core::types::Transaction;
use strangecoin_net::discovery::addr_gossip::AddrEntry;
use strangecoin_net::peer::scoring::AcpViolation;
use strangecoin_net::rate_limiter::{BucketSpec, RateLimitConfig};
use strangecoin_net::{
    AcpEnvelope, AcpHandler, AcpHandlerConfig, AcpKind, AcpMessage, AcpNegotiate, AcpQuote,
    AcpRevoke, AcpSettle, AcpTask, AgentId, BlockchainView, EscrowAction, EscrowSettlement,
    MemoryNonceStore, MessageType, NodeId, NodeKeypair, PeerManager, PeerManagerConfig,
    RateLimiter, ScoringConfig,
};

// The contract source (WAT + selectors + its own unit tests) lives in the
// vm-wasm example; including it here keeps one implementation shared by the
// deploy path, the smoke `main` (cfg'd out under test) and the example's
// test matrix.
#[path = "../crates/strangecoin-vm-wasm/examples/escrow_vault.rs"]
mod escrow_vault;

// ---------------------------------------------------------------- identities

/// Fixed agent identities: A (assigner, funded), B (performer, receives
/// escrow releases). Deterministic seeds — no RNG anywhere.
fn agent_a() -> (String, secp256k1::SecretKey) {
    agent_from_seed([0xA1; 32])
}

fn agent_b() -> (String, secp256k1::SecretKey) {
    agent_from_seed([0xB2; 32])
}

fn agent_from_seed(seed: [u8; 32]) -> (String, secp256k1::SecretKey) {
    let sk = secp256k1::SecretKey::from_slice(&seed).expect("fixed seed");
    let pk = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), &sk);
    let addr = encode_address(&pk, CHAIN_ID_REGTEST).expect("regtest address");
    (addr, sk)
}

fn agent_id(sk: &secp256k1::SecretKey) -> AgentId {
    let pk = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), sk);
    AgentId(pk.serialize())
}

fn sign_escrow_digest(id: &[u8; 8], sk: &secp256k1::SecretKey) -> [u8; 64] {
    use secp256k1::Message;
    let digest = escrow::escrow_signature_message(id);
    let msg = Message::from_digest(digest);
    secp256k1::Secp256k1::new()
        .sign_ecdsa(&msg, sk)
        .serialize_compact()
}

fn node_id(seed: u8) -> NodeId {
    NodeKeypair::from_seed([seed; 32]).node_id()
}

// ---------------------------------------------------------------- settlement

/// Test-side settlement adapter: submits the real `EscrowVault.release` /
/// `.refund` call transaction through the facade and mines it — the
/// "on-chain release/refund" half of the e2e flows (production wires
/// `NoopEscrowVault` until AI-P04, ADR-0030 amendment).
struct ChainEscrowVault {
    bc: BlockchainFacade,
    vault: String,
    submitter: String,
    submitter_sk: secp256k1::SecretKey,
}

impl ChainEscrowVault {
    fn submit(&self, data: Vec<u8>) -> Result<(), String> {
        // Mempool nonces are 1-based (`validate_nonce`, BUG-S0-025).
        let nonce = self.bc.get_nonce(&self.submitter) + 1;
        let mut tx = Transaction::call(
            &self.submitter,
            &self.vault,
            data,
            0,
            300_000,
            nonce,
            CHAIN_ID_REGTEST,
        );
        sign_transaction(&mut tx, &self.submitter_sk);
        let id = txid(&tx);
        self.bc.apply_tx(tx).map_err(|err| err.to_string())?;
        mine_current(&self.bc);
        // Fail loudly if the contract reverted on-chain: a Failed receipt
        // must never masquerade as a settled escrow (the balance asserts
        // downstream would otherwise be the only — opaque — signal).
        let receipts = self.bc.tip_receipts();
        match receipts.iter().find(|r| r.txid == id) {
            Some(receipt) if receipt.status == strangecoin_core::vm::TxStatus::Success => Ok(()),
            Some(receipt) => Err(format!(
                "escrow tx reverted on-chain: status={:?}, gas_used={}",
                receipt.status, receipt.gas_used
            )),
            None => Err("escrow tx missing from the mined tip".into()),
        }
    }
}

impl EscrowSettlement for ChainEscrowVault {
    fn release(&self, action: &EscrowAction) -> Result<(), String> {
        let EscrowAction::Release {
            id_bytes,
            signature_a,
            signature_b,
            ..
        } = action
        else {
            return Err("release adapter got a refund action".into());
        };
        self.submit(escrow::encode_release(id_bytes, signature_a, signature_b))
    }

    fn refund(&self, action: &EscrowAction) -> Result<(), String> {
        let EscrowAction::Refund {
            id_bytes,
            signature_a,
            signature_b,
            ..
        } = action
        else {
            return Err("refund adapter got a release action".into());
        };
        self.submit(escrow::encode_refund(
            id_bytes,
            signature_a,
            signature_b.as_ref(),
        ))
    }
}

// ---------------------------------------------------------------- environment

/// One test node: facade + shared peer manager + the two directional ACP
/// handlers (A-side processes B→A frames, B-side processes A→B frames).
struct AcpEnv {
    _dir: TestDir,
    bc: BlockchainFacade,
    addr_a: String,
    sk_a: secp256k1::SecretKey,
    addr_b: String,
    sk_b: secp256k1::SecretKey,
    vault: Option<String>,
    /// Node identity of agent A's side (sender of every A→B frame here).
    node_a: NodeId,
    /// Handler receiving A→B messages (agent B's node).
    handler_b: AcpHandler,
    /// Handler receiving B→A messages (agent A's node).
    handler_a: AcpHandler,
    pm: Arc<PeerManager>,
}

impl AcpEnv {
    /// Fresh regtest facade, funded agent A, registered sender peer.
    fn new(tag: &str) -> Self {
        let dir = TestDir::new(tag);
        let bc = create_test_blockchain(dir.path());
        let (addr_a, sk_a) = agent_a();
        let (addr_b, sk_b) = agent_b();
        assert!(
            bc.grant_initial_balance_to_first_wallet(&addr_a)
                .expect("grant applies"),
            "grant block created"
        );

        let node_a = node_id(0xA1);
        let pm = Arc::new(PeerManager::new(
            PeerManagerConfig::default(),
            NodeKeypair::from_seed([0xEE; 32]),
        ));
        pm.learn_addrs(vec![AddrEntry {
            node_id: node_a,
            addr: "10.99.0.1:8081".parse().expect("addr"),
        }]);

        // Views built before the struct literal moves `bc` into its field.
        let view_b = facade_view(&bc);
        let view_a = facade_view(&bc);
        let pm2 = Arc::clone(&pm);

        AcpEnv {
            _dir: dir,
            bc,
            addr_a,
            sk_a,
            addr_b,
            sk_b,
            vault: None,
            node_a,
            handler_b: AcpHandler::new(
                view_b,
                Arc::new(MemoryNonceStore::new()),
                Arc::new(NoopVault),
                Arc::clone(&pm2),
                AcpHandlerConfig {
                    network_id: CHAIN_ID_REGTEST,
                    escrow_contract: None,
                },
            ),
            handler_a: AcpHandler::new(
                view_a,
                Arc::new(MemoryNonceStore::new()),
                Arc::new(NoopVault),
                Arc::clone(&pm2),
                AcpHandlerConfig {
                    network_id: CHAIN_ID_REGTEST,
                    escrow_contract: None,
                },
            ),
            pm: pm2,
        }
    }

    /// Deploy the EscrowVault (agent A) and rebuild both handlers with the
    /// deployed address + chain settlement.
    ///
    /// The deploy rides the **craft path with a 0-based nonce** (the
    /// block-apply convention): `apply_create` derives the contract
    /// address from the pre-state nonce, and `collect_block_codes` — the
    /// facade's code registry — keys by `create.nonce`, so only a crafted
    /// (0-based) create lands code and address on the same key. Mempool
    /// creates are 1-based (`validate_nonce`, BUG-S0-025) and miskey the
    /// registry (latent off-by-one recorded in the AI-P03 commit); Calls
    /// are unaffected, so deposits/settlements still go through the
    /// mempool + `mine_current` as production does.
    fn deploy_vault(&mut self) -> String {
        let pre_nonce = self.bc.get_nonce(&self.addr_a);
        let wat = escrow_vault::escrow_vault_wat();
        let mut create = Transaction::create(
            &self.addr_a,
            wat.into_bytes(),
            vec![],
            0,
            pre_nonce,
            CHAIN_ID_REGTEST,
        );
        sign_transaction(&mut create, &self.sk_a);
        craft_block_with(&self.bc, vec![create]);
        let vault =
            derive_contract_address(&self.addr_a, pre_nonce, CHAIN_ID_REGTEST).expect("vault addr");
        // The address string alone proves nothing — a wrong derivation
        // turns every later Call into a silent transfer-only no-op (the
        // exact bug this assert existed to catch).
        let account = self
            .bc
            .get_account(&vault)
            .expect("deployed vault account must exist");
        assert!(
            account.is_contract(),
            "derived vault address must hold the deployed contract"
        );

        // Everything cloned before the handler rebuilds (the settlement
        // adapter shares the facade — `inner: Arc<RwLock<..>>`). The
        // closure's last use ends before the assignments, so `self` is
        // free for the field writes.
        let view_b = facade_view(&self.bc);
        let view_a = facade_view(&self.bc);
        let make_settlement = || ChainEscrowVault {
            bc: self.bc.clone(),
            vault: vault.clone(),
            submitter: self.addr_a.clone(),
            submitter_sk: self.sk_a,
        };
        let settle_b = make_settlement();
        let settle_a = make_settlement();
        let pm = Arc::clone(&self.pm);
        self.handler_b = AcpHandler::new(
            view_b,
            Arc::new(MemoryNonceStore::new()),
            Arc::new(settle_b),
            Arc::clone(&pm),
            AcpHandlerConfig {
                network_id: CHAIN_ID_REGTEST,
                escrow_contract: Some(vault.clone()),
            },
        );
        self.handler_a = AcpHandler::new(
            view_a,
            Arc::new(MemoryNonceStore::new()),
            Arc::new(settle_a),
            Arc::clone(&pm),
            AcpHandlerConfig {
                network_id: CHAIN_ID_REGTEST,
                escrow_contract: Some(vault.clone()),
            },
        );
        self.vault = Some(vault.clone());
        vault
    }

    /// Mine a deposit of `amount` SC from agent A into the vault with
    /// `deadline_block`; returns `(escrow_txid, deadline_block)`.
    fn deposit(&self, amount: u64, deadline_block: u64) -> ([u8; 32], u64) {
        let vault = self.vault.as_ref().expect("deploy first");
        let calldata = escrow::encode_deposit(&EscrowDepositParams {
            agent_a: &self.addr_a,
            agent_b: &self.addr_b,
            pk_a: agent_id(&self.sk_a).0,
            pk_b: agent_id(&self.sk_b).0,
            amount,
            deadline_block,
        })
        .expect("deposit calldata");
        let nonce = self.bc.get_nonce(&self.addr_a) + 1;
        let mut call = Transaction::call(
            &self.addr_a,
            vault,
            calldata,
            amount,
            300_000,
            nonce,
            CHAIN_ID_REGTEST,
        );
        sign_transaction(&mut call, &self.sk_a);
        let id = txid(&call);
        self.bc.apply_tx(call).expect("deposit accepted");
        mine_current(&self.bc);
        (id, deadline_block)
    }

    /// Crafted coinbase-only blocks until `tip_height >= target`
    /// (`test_support::advance_height` — the pure-core `craft_child`
    /// cannot replay a prefix that contains our `Create` deploy; heights
    /// move without wall time either way).
    fn advance_to(&self, target_height: u64) {
        while (self.bc.chain_len() as u64).saturating_sub(1) < target_height {
            advance_height(&self.bc);
        }
    }
}

fn facade_view(bc: &BlockchainFacade) -> Arc<dyn BlockchainView> {
    Arc::new(bc.clone())
}

/// Calldata of a mined escrow deposit (panic = the txid is not a Call —
/// every ACP escrow test feeds its own deposit here).
fn deposit_data(bc: &BlockchainFacade, escrow_txid: &[u8; 32]) -> Vec<u8> {
    match bc.get_tx(escrow_txid).expect("deposit tx mined") {
        Transaction::Call(call) => call.data,
        other => panic!("escrow txid must reference a call tx, got {other:?}"),
    }
}

/// Placeholder settlement for handler variants that never settle (kept so
/// `AcpEnv::new` can build handlers before the vault exists).
struct NoopVault;

impl EscrowSettlement for NoopVault {
    fn release(&self, _action: &EscrowAction) -> Result<(), String> {
        Err("vault not deployed".into())
    }
    fn refund(&self, _action: &EscrowAction) -> Result<(), String> {
        Err("vault not deployed".into())
    }
}

// ---------------------------------------------------------------- message builders

fn negotiate(nonce: u64, a: &AgentId, b: &AgentId, quote: Option<AcpQuote>) -> AcpMessage {
    AcpMessage::Negotiate(AcpNegotiate {
        agent_id_a: *a,
        agent_id_b: *b,
        agent_nonce: nonce,
        task_spec: serde_json::json!({"description": "summarize document X"}),
        quote,
    })
}

fn sign_wire(message: AcpMessage, sender: &secp256k1::SecretKey) -> Vec<u8> {
    let id = agent_id(sender);
    AcpEnvelope::sign(message, id, sender)
        .expect("envelope signs")
        .encode_wire()
}

// ================================================================ КГ tests

/// КГ #1 / spec §4.1: negotiate → quote → on-chain deposit → task →
/// settle → real `EscrowVault.release` paying agent B.
#[test]
fn acp_negotiate_task_settle_e2e() {
    let mut env = AcpEnv::new("acp_e2e");
    let vault = env.deploy_vault();
    let a = agent_id(&env.sk_a);
    let b = agent_id(&env.sk_b);

    // 1-2. Negotiate request (A→B) and quote response (B→A).
    let accepted = env
        .handler_b
        .handle(
            &sign_wire(negotiate(1, &a, &b, None), &env.sk_a),
            env.node_a,
        )
        .expect("negotiate request accepted");
    assert_eq!(accepted.kind, AcpKind::Negotiate);

    let quote = AcpQuote {
        amount_sc: 50,
        deadline_blocks: 1000,
    };
    // Spec §3.1: the pair fields stay (A=assigner, B=performer) in both
    // directions — only the sender changes. Signed by B, delivered to A's
    // node.
    env.handler_a
        .handle(
            &sign_wire(negotiate(1, &a, &b, Some(quote.clone())), &env.sk_b),
            node_id(0xB2),
        )
        .expect("quote response accepted");
    assert_eq!(env.handler_a.quote_for(a, b), Some(quote.clone()));

    // 3. On-chain escrow deposit (50 SC, deadline 5000).
    let (escrow_txid, deadline) = env.deposit(quote.amount_sc, 5000);
    assert_eq!(env.bc.get_balance(&env.addr_a), 10_000 - 50);
    assert_eq!(env.bc.get_balance(&vault), 50);

    // 4. Task assignment referencing the deposit.
    let task = AcpMessage::Task(AcpTask {
        agent_id_a: a,
        agent_id_b: b,
        agent_nonce: 2,
        task_spec: serde_json::json!({"description": "summarize document X"}),
        escrow_txid,
        deadline_block: deadline,
    });
    let accepted = env
        .handler_b
        .handle(&sign_wire(task, &env.sk_a), env.node_a)
        .expect("task accepted (escrow #24 verified)");
    assert_eq!(accepted.kind, AcpKind::Task);
    assert_eq!(accepted.escrow_txid, Some(escrow_txid));

    // 5-6. Settle → the adapter submits the real release tx on-chain.
    let id_bytes = escrow::escrow_id_bytes(&deposit_data(&env.bc, &escrow_txid));
    assert_eq!(
        accepted.escrow_id,
        Some(u64::from_le_bytes(id_bytes)),
        "handler derives the same escrow id as the contract"
    );
    let settle = AcpMessage::Settle(AcpSettle {
        agent_id_a: a,
        agent_id_b: b,
        agent_nonce: 3,
        escrow_txid,
        settle_signature_a: sign_escrow_digest(&id_bytes, &env.sk_a),
        settle_signature_b: sign_escrow_digest(&id_bytes, &env.sk_b),
    });
    let accepted = env
        .handler_b
        .handle(&sign_wire(settle, &env.sk_a), env.node_a)
        .expect("settle accepted + release submitted");
    assert_eq!(accepted.kind, AcpKind::Settle);

    // 7. On-chain effect: B got the escrow, the vault is empty again.
    assert_eq!(env.bc.get_balance(&env.addr_b), 50, "release pays agent B");
    assert_eq!(env.bc.get_balance(&vault), 0, "vault drained to zero");
    assert_eq!(env.bc.get_balance(&env.addr_a), 10_000 - 50);
}

/// КГ #2 / spec §4.2: B never answers, the deadline passes, A revokes with
/// `deadline_passed = true` → on-chain refund to A.
#[test]
fn acp_revoke_after_timeout() {
    let mut env = AcpEnv::new("acp_timeout");
    env.deploy_vault();
    let a = agent_id(&env.sk_a);
    let b = agent_id(&env.sk_b);

    let (escrow_txid, deadline) = env.deposit(50, 5);
    env.handler_b
        .handle(
            &sign_wire(
                AcpMessage::Task(AcpTask {
                    agent_id_a: a,
                    agent_id_b: b,
                    agent_nonce: 1,
                    task_spec: serde_json::json!({"description": "t"}),
                    escrow_txid,
                    deadline_block: deadline,
                }),
                &env.sk_a,
            ),
            env.node_a,
        )
        .expect("task accepted");

    // B silent; walk the chain past the deadline (tip > 5).
    env.advance_to(deadline + 1);

    let id_bytes = escrow::escrow_id_bytes(&deposit_data(&env.bc, &escrow_txid));
    let revoke = AcpMessage::Revoke(AcpRevoke {
        agent_id_a: a,
        agent_id_b: b,
        agent_nonce: 2,
        escrow_txid,
        revoke_signature_a: sign_escrow_digest(&id_bytes, &env.sk_a),
        revoke_signature_b: None,
        deadline_passed: true,
    });
    env.handler_b
        .handle(&sign_wire(revoke, &env.sk_a), env.node_a)
        .expect("timeout revoke accepted + refund submitted");

    let vault = env.vault.as_ref().unwrap();
    assert_eq!(
        env.bc.get_balance(&env.addr_a),
        10_000,
        "timeout refund returns the escrow to A"
    );
    assert_eq!(env.bc.get_balance(vault), 0);
}

/// КГ #3 / invariant #24 + V-45: SETTLE referencing a txid that is not in
/// the chain → rejected and the peer banned (scored: `MissingEscrowTx`).
#[test]
fn acp_escrow_fraud_rejected() {
    let mut env = AcpEnv::new("acp_fraud");
    env.deploy_vault();
    let a = agent_id(&env.sk_a);
    let b = agent_id(&env.sk_b);

    // One violation unit = the full penalty weight: a single −0.1 crosses
    // the ban threshold (the net_peer_scoring tuning pattern).
    let scoring = ScoringConfig {
        w_violation: 1.0,
        violation_full_weight: 1,
        ..ScoringConfig::default()
    };
    let cfg = PeerManagerConfig {
        scoring,
        ..PeerManagerConfig::default()
    };
    let pm = Arc::new(PeerManager::new(cfg, NodeKeypair::from_seed([0xEF; 32])));
    pm.learn_addrs(vec![AddrEntry {
        node_id: env.node_a,
        addr: "10.99.0.2:8081".parse().expect("addr"),
    }]);
    let handler = AcpHandler::new(
        facade_view(&env.bc),
        Arc::new(MemoryNonceStore::new()),
        Arc::new(NoopVault),
        Arc::clone(&pm),
        AcpHandlerConfig {
            network_id: CHAIN_ID_REGTEST,
            // No deposit exists — the missing-txid check fires before the
            // address is ever compared, so any configured value works.
            escrow_contract: Some(vault_placeholder()),
        },
    );

    let settle = AcpMessage::Settle(AcpSettle {
        agent_id_a: a,
        agent_id_b: b,
        agent_nonce: 1,
        escrow_txid: [0xFE; 32], // never mined
        settle_signature_a: [0x11; 64],
        settle_signature_b: [0x22; 64],
    });
    let err = handler
        .handle(&sign_wire(settle, &env.sk_a), env.node_a)
        .expect_err("fake txid must be rejected");
    assert!(
        matches!(
            err,
            strangecoin_net::AcpReject::Escrow(AcpViolation::MissingEscrowTx)
        ),
        "unexpected reject: {err}"
    );
    let ban = pm.get_ban(&env.node_a).expect("peer banned (V-45)");
    assert!(
        ban.reason == strangecoin_net::BanReason::ProtocolViolation,
        "banned through the ACP violation channel: {:?}",
        ban.reason
    );
    let record = pm.get_peer(&env.node_a).expect("record");
    assert!(
        record.violation_count >= 2,
        "MissingEscrowTx = −0.1 = 2 violation units, got {}",
        record.violation_count
    );
}

/// КГ #4 / invariant #25: byte-identical replay of a delivered NEGOTIATE
/// (same nonce) → `Replay`, the nonce window never moves backwards.
#[test]
fn acp_replay_rejected() {
    let env = AcpEnv::new("acp_replay");
    let a = agent_id(&env.sk_a);
    let b = agent_id(&env.sk_b);

    let wire = sign_wire(negotiate(7, &a, &b, None), &env.sk_a);
    env.handler_b
        .handle(&wire, env.node_a)
        .expect("first delivery accepted");

    let err = env
        .handler_b
        .handle(&wire, env.node_a)
        .expect_err("stale nonce must be rejected");
    assert!(
        matches!(err, strangecoin_net::AcpReject::Replay { last: 7, got: 7 }),
        "unexpected reject: {err}"
    );
    // The peer pays a protocol violation for the replay attempt.
    let record = env.pm.get_peer(&env.node_a).expect("record");
    assert!(
        record.violation_count >= 1,
        "replay charged as a protocol violation"
    );

    // A fresh higher nonce still passes (the window is monotonic, not
    // closed).
    env.handler_b
        .handle(
            &sign_wire(negotiate(8, &a, &b, None), &env.sk_a),
            env.node_a,
        )
        .expect("next nonce accepted");
}

/// КГ #5 / V-44: an `ACP_NEGOTIATE` flood drains the Stage 2 sub-bucket —
/// exactly the burst is admitted, the refusals escalate through
/// `apply_rate_verdict` into a peer ban. Clock injected (`t0`): zero refill,
/// zero sleeps.
#[test]
fn acp_spam_rate_limited() {
    let env = AcpEnv::new("acp_spam");
    let a = agent_id(&env.sk_a);
    let b = agent_id(&env.sk_b);

    let limiter = RateLimiter::new(RateLimitConfig {
        acp_negotiate: BucketSpec::new(100, 10),
        ..RateLimitConfig::default()
    });
    let addr: std::net::SocketAddr = "10.99.0.3:8081".parse().expect("addr");
    let t0 = Instant::now();

    let mut admitted = 0u32;
    let mut refusals = 0u32;
    for nonce in 1..=300u64 {
        let wire = sign_wire(negotiate(nonce, &a, &b, None), &env.sk_a);
        let verdict = limiter.check_rate_at(t0, addr, MessageType::AcpNegotiate);
        match &verdict {
            Ok(()) => {
                env.handler_b
                    .handle(&wire, env.node_a)
                    .expect("admitted negotiate accepted");
                admitted += 1;
            }
            Err(_) => {
                refusals += 1;
                env.pm
                    .apply_rate_verdict(env.node_a, &verdict, limiter.config().ban_duration);
            }
        }
    }

    assert_eq!(
        admitted, 100,
        "acp_negotiate bucket admits exactly its burst (spec §7)"
    );
    assert!(refusals >= 200, "the flood beyond the burst is refused");
    assert!(
        env.pm.get_ban(&env.node_a).is_some(),
        "spam escalates to a peer ban (V-44)"
    );
    // The window stopped at the burst edge: nonces 101..300 never reached
    // the handler (they were refused by the limiter), so the first delivery
    // of nonce 101 is fresh and accepted — while a redelivery of the last
    // admitted nonce (100) is a replay.
    env.handler_b
        .handle(
            &sign_wire(negotiate(101, &a, &b, None), &env.sk_a),
            env.node_a,
        )
        .expect("nonce 101 first delivery accepted — the refused flood never touched the window");
    env.handler_b
        .handle(
            &sign_wire(negotiate(100, &a, &b, None), &env.sk_a),
            env.node_a,
        )
        .expect_err("redelivered nonce 100 is a replay");
}

// ================================================================ spec §11 extras

/// Spec §11: mutual cancel before the deadline (both signatures, no height
/// constraint) refunds A through the real on-chain path.
#[test]
fn acp_revoke_mutual_cancel() {
    let mut env = AcpEnv::new("acp_mutual");
    env.deploy_vault();
    let a = agent_id(&env.sk_a);
    let b = agent_id(&env.sk_b);

    let (escrow_txid, deadline) = env.deposit(50, 5000);
    env.handler_b
        .handle(
            &sign_wire(
                AcpMessage::Task(AcpTask {
                    agent_id_a: a,
                    agent_id_b: b,
                    agent_nonce: 1,
                    task_spec: serde_json::json!({"description": "t"}),
                    escrow_txid,
                    deadline_block: deadline,
                }),
                &env.sk_a,
            ),
            env.node_a,
        )
        .expect("task accepted");

    let id_bytes = escrow::escrow_id_bytes(&deposit_data(&env.bc, &escrow_txid));
    let revoke = AcpMessage::Revoke(AcpRevoke {
        agent_id_a: a,
        agent_id_b: b,
        agent_nonce: 2,
        escrow_txid,
        revoke_signature_a: sign_escrow_digest(&id_bytes, &env.sk_a),
        revoke_signature_b: Some(sign_escrow_digest(&id_bytes, &env.sk_b)),
        deadline_passed: false,
    });
    // Sent by B (either party may initiate a mutual cancel).
    env.handler_a
        .handle(&sign_wire(revoke, &env.sk_b), node_id(0xB2))
        .expect("mutual revoke accepted + refund submitted");

    let vault = env.vault.as_ref().unwrap();
    assert_eq!(env.bc.get_balance(&env.addr_a), 10_000, "mutual refund");
    assert_eq!(env.bc.get_balance(vault), 0);
}

/// Spec §11: `deadline_passed = true` before the deposit deadline →
/// rejected as `EscrowDeadline` (−0.1), nothing is submitted on-chain.
#[test]
fn acp_premature_revoke_rejected() {
    let mut env = AcpEnv::new("acp_premature");
    env.deploy_vault();
    let a = agent_id(&env.sk_a);
    let b = agent_id(&env.sk_b);

    let (escrow_txid, _deadline) = env.deposit(50, 5000);
    let id_bytes = escrow::escrow_id_bytes(&deposit_data(&env.bc, &escrow_txid));
    let revoke = AcpMessage::Revoke(AcpRevoke {
        agent_id_a: a,
        agent_id_b: b,
        agent_nonce: 1,
        escrow_txid,
        revoke_signature_a: sign_escrow_digest(&id_bytes, &env.sk_a),
        revoke_signature_b: None,
        deadline_passed: true,
    });
    let err = env
        .handler_b
        .handle(&sign_wire(revoke, &env.sk_a), env.node_a)
        .expect_err("premature revoke must be rejected");
    assert!(
        matches!(
            err,
            strangecoin_net::AcpReject::Escrow(AcpViolation::EscrowDeadline)
        ),
        "unexpected reject: {err}"
    );
    let record = env.pm.get_peer(&env.node_a).expect("record");
    assert!(
        record.violation_count >= 2,
        "premature revoke = −0.1 = 2 units, got {}",
        record.violation_count
    );
    // Funds untouched — the vault still holds the escrow.
    let vault = env.vault.as_ref().unwrap();
    assert_eq!(env.bc.get_balance(vault), 50);
    assert_eq!(env.bc.get_balance(&env.addr_a), 10_000 - 50);
}

/// Spec §11: SETTLE with a forged agent-B signature over the *real* escrow →
/// `ForgedEscrowSignature` (−0.3) and the peer banned under tuned weights.
#[test]
fn acp_forged_settle_signature_rejected() {
    let mut env = AcpEnv::new("acp_forged");
    env.deploy_vault();
    let a = agent_id(&env.sk_a);
    let b = agent_id(&env.sk_b);
    let (escrow_txid, _deadline) = env.deposit(50, 5000);

    // Same one-unit-one-penalty tuning as the fraud test.
    // One violation unit = the full penalty weight: a single −0.1 crosses
    // the ban threshold (the net_peer_scoring tuning pattern).
    let scoring = ScoringConfig {
        w_violation: 1.0,
        violation_full_weight: 1,
        ..ScoringConfig::default()
    };
    let cfg = PeerManagerConfig {
        scoring,
        ..PeerManagerConfig::default()
    };
    let pm = Arc::new(PeerManager::new(cfg, NodeKeypair::from_seed([0xFD; 32])));
    pm.learn_addrs(vec![AddrEntry {
        node_id: env.node_a,
        addr: "10.99.0.4:8081".parse().expect("addr"),
    }]);
    let handler = AcpHandler::new(
        facade_view(&env.bc),
        Arc::new(MemoryNonceStore::new()),
        Arc::new(NoopVault),
        Arc::clone(&pm),
        AcpHandlerConfig {
            network_id: CHAIN_ID_REGTEST,
            escrow_contract: env.vault.clone(),
        },
    );

    let id_bytes = escrow::escrow_id_bytes(&deposit_data(&env.bc, &escrow_txid));
    let mut forged_b = sign_escrow_digest(&id_bytes, &env.sk_b);
    forged_b[3] ^= 0x5A;
    let settle = AcpMessage::Settle(AcpSettle {
        agent_id_a: a,
        agent_id_b: b,
        agent_nonce: 1,
        escrow_txid,
        settle_signature_a: sign_escrow_digest(&id_bytes, &env.sk_a),
        settle_signature_b: forged_b,
    });
    let err = handler
        .handle(&sign_wire(settle, &env.sk_a), env.node_a)
        .expect_err("forged sig_b must be rejected");
    assert!(
        matches!(
            err,
            strangecoin_net::AcpReject::Escrow(AcpViolation::ForgedEscrowSignature)
        ),
        "unexpected reject: {err}"
    );
    assert!(
        pm.get_ban(&env.node_a).is_some(),
        "forged settlement signature escalates to a ban (V-45)"
    );
    // On-chain funds untouched.
    let vault = env.vault.as_ref().unwrap();
    assert_eq!(env.bc.get_balance(vault), 50);
    assert_eq!(env.bc.get_balance(&env.addr_b), 0);
}

// ---------------------------------------------------------------- small utils

/// A valid-looking bech32 placeholder for configs whose contract address is
/// never reached (missing-txid check fires first). Derived from agent A's
/// real identity so it decodes as a genuine regtest address.
fn vault_placeholder() -> String {
    let (addr, _sk) = agent_from_seed([0xC3; 32]);
    // Any address works here — re-label it as a contract by derivation.
    derive_contract_address(&addr, 0, CHAIN_ID_REGTEST).expect("placeholder")
}
