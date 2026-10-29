//! S2-P22 — Stage 2 network integration helpers.
//!
//! Provides the simulated network the DoD propagation tests run on:
//! [`SimulatedNetwork`] + [`MockPeer`] with a manual deterministic clock
//! («simulated clock, no real network», prompt S2-P22 task 1), the
//! [`simulate_gossip_propagation`] flood model with per-peer arrival times,
//! an in-memory [`MockView`] (`BlockchainView`), and the shared tx/block
//! builders + in-memory Noise handshake pair the six `net_*` test files use.
//!
//! **Determinism** (Stage 2 checklist #4/#5): the clock is a manual offset
//! from construction time, peers are kept sorted by `node_id`, and the
//! propagation model has no RNG — the same inputs always produce the same
//! arrival times.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use strangecoin_core::consensus::U256;
use strangecoin_core::serialize::{block_hash, merkle_root, txid};
use strangecoin_core::types::{
    AccountState, Block, ChainSnapshot, CreateTx, Transaction, TransferTx,
};
use strangecoin_net::protocol::hello::STAGE2_CAPABILITIES;
use strangecoin_net::{
    BlockchainView, NodeId, NodeKeypair, NoiseHandshake, NoiseTransport, PeerManager,
    PeerManagerConfig,
};

/// Simulated per-hop delay in the gossip flood model (deterministic, not a
/// wall-clock read).
pub const GOSSIP_HOP_DELAY: Duration = Duration::from_millis(10);

/// Gossip fan-out window: each infected peer relays to the next
/// `GOSSIP_FANOUT` peers in `node_id` order (topology by sorted index — no
/// RNG, see the determinism note above).
pub const GOSSIP_FANOUT: usize = 5;

const HANDSHAKE_BUF: usize = 512;

// ----------------------------------------------------------------- MockPeer

/// A simulated peer: deterministic identity, synthetic address, the Stage 2
/// capability set.
pub struct MockPeer {
    pub node_id: NodeId,
    pub identity: NodeKeypair,
    pub addr: SocketAddr,
    pub capabilities: u32,
}

impl MockPeer {
    /// `seed` derives the identity; `last` makes the address unique inside
    /// the test's private `10.42.0.0/24`.
    pub fn new(seed: [u8; 32], last: u8) -> Self {
        let identity = NodeKeypair::from_seed(seed);
        Self {
            node_id: identity.node_id(),
            identity,
            addr: SocketAddr::new(IpAddr::from([10, 42, 0, last]), 8081),
            capabilities: STAGE2_CAPABILITIES,
        }
    }
}

// ---------------------------------------------------------- SimulatedNetwork

/// In-memory network of [`MockPeer`]s with a manual clock.
///
/// The origin node is tracked separately (it creates the tx); `peers` holds
/// the propagation targets, sorted by `node_id` for a deterministic
/// simulation order.
pub struct SimulatedNetwork {
    origin: NodeId,
    peers: Vec<MockPeer>,
    base: Instant,
}

impl SimulatedNetwork {
    /// A network with `count` simulated peers (seeds `[1;32]`, `[2;32]`, …)
    /// around an origin derived from `origin_seed`.
    pub fn with_peers(origin_seed: [u8; 32], count: u8) -> Self {
        let origin = NodeKeypair::from_seed(origin_seed).node_id();
        let mut peers: Vec<MockPeer> = (1..=count).map(|i| MockPeer::new([i; 32], i)).collect();
        peers.sort_by_key(|peer| peer.node_id);
        Self {
            origin,
            peers,
            base: Instant::now(),
        }
    }

    /// The node that creates the tx.
    pub fn origin(&self) -> NodeId {
        self.origin
    }

    /// Simulation targets, sorted by `node_id`.
    pub fn peers(&self) -> &[MockPeer] {
        &self.peers
    }

    /// The deterministic clock: `base + offset`.
    pub fn at(&self, offset: Duration) -> Instant {
        self.base + offset
    }
}

/// Flood the tx through `network` and return each peer's first-arrival time
/// on the simulated clock.
///
/// Topology: the origin connects directly to the first [`GOSSIP_FANOUT`]
/// peers in `node_id` order; every infected peer relays to the next
/// [`GOSSIP_FANOUT`] peers after its own position. First arrival wins
/// (INV dedup semantics) — same inputs, same arrival times.
///
/// # Panics
/// Panics if `tx_origin` is not the network's origin node.
pub fn simulate_gossip_propagation(
    tx_origin: NodeId,
    network: &SimulatedNetwork,
) -> Vec<(NodeId, Instant)> {
    assert_eq!(
        tx_origin,
        network.origin(),
        "tx_origin must be the network's origin"
    );

    let n = network.peers().len();

    let mut arrivals: Vec<(NodeId, Instant)> = Vec::with_capacity(n);
    let mut infected: HashSet<usize> = HashSet::with_capacity(n);
    let mut queue: VecDeque<(usize, u32)> = VecDeque::new();

    // Origin's direct connections (depth 1, all within one hop).
    for i in 0..GOSSIP_FANOUT.min(n) {
        infected.insert(i);
        arrivals.push((network.peers()[i].node_id, network.at(GOSSIP_HOP_DELAY)));
        queue.push_back((i, 1));
    }

    // Relay rounds: each infected peer `i` reaches `i+1 ..= i+FANOUT`.
    while let Some((i, depth)) = queue.pop_front() {
        let next_depth = depth + 1;
        for j in (i + 1)..=(i + GOSSIP_FANOUT) {
            if j >= n {
                break;
            }
            if infected.insert(j) {
                arrivals.push((
                    network.peers()[j].node_id,
                    network.at(GOSSIP_HOP_DELAY * next_depth),
                ));
                queue.push_back((j, next_depth));
            }
        }
    }

    // The model must reach every peer (the DoD assertion lives in the test;
    // here we only guarantee no silent partial result).
    debug_assert_eq!(arrivals.len(), n, "every peer eventually hears the tx");
    arrivals
}

// ----------------------------------------------------------------- MockView

/// In-memory `BlockchainView`: a controllable mempool plus a chain of full
/// blocks — the receiver/broadcaster fixture for gossip, compact-block and
/// Erlay tests.
#[derive(Default)]
pub struct MockView {
    pub mempool: HashMap<[u8; 32], Transaction>,
    pub blocks: HashMap<String, Block>,
}

impl MockView {
    pub fn with_mempool(txs: Vec<Transaction>) -> Self {
        let mut mempool = HashMap::new();
        for tx in txs {
            mempool.insert(txid(&tx), tx);
        }
        Self {
            mempool,
            blocks: HashMap::new(),
        }
    }

    pub fn insert_tx(&mut self, tx: Transaction) {
        self.mempool.insert(txid(&tx), tx);
    }

    pub fn insert_block(&mut self, block: Block) {
        self.blocks.insert(block.hash.clone(), block);
    }
}

impl BlockchainView for MockView {
    fn tip_hash(&self) -> String {
        self.blocks
            .values()
            .max_by_key(|b| b.index)
            .map(|b| b.hash.clone())
            .unwrap_or_default()
    }

    fn chain_len(&self) -> usize {
        self.blocks.len()
    }

    fn adopt_candidate(
        &self,
        _candidate_chain: Vec<Block>,
        _candidate_balances: Option<HashMap<String, AccountState>>,
        _candidate_mempool_txs: Vec<Transaction>,
        _candidate_difficulty: u32,
    ) -> Result<bool, String> {
        Ok(false)
    }

    fn apply_tx(&self, _transaction: Transaction) -> Result<(), String> {
        Ok(())
    }

    fn add_block(&self, _block: Block) -> Result<(), String> {
        Ok(())
    }

    fn chain_contains_tx(&self, _tx: &Transaction) -> bool {
        false
    }

    fn mempool_contains(&self, txid: &[u8; 32]) -> bool {
        self.mempool.contains_key(txid)
    }

    fn mempool_tx(&self, txid: &[u8; 32]) -> Option<Transaction> {
        self.mempool.get(txid).cloned()
    }

    fn mempool_txs(&self) -> Vec<Transaction> {
        self.mempool.values().cloned().collect()
    }

    fn block_by_hash(&self, block_hash: &str) -> Option<Block> {
        self.blocks.get(block_hash).cloned()
    }

    fn save_state(&self) {}

    fn chain_contains_block(&self, block_hash: &str) -> bool {
        self.blocks.contains_key(block_hash)
    }

    fn snapshot_wire(&self) -> ChainSnapshot {
        ChainSnapshot {
            chain: Vec::new(),
            balances: HashMap::new(),
            difficulty: 0,
            mempool_txs: self.mempool.values().cloned().collect(),
            total_work: U256::default(),
        }
    }

    fn headers_from_height(
        &self,
        _from_height: u64,
        _max: usize,
    ) -> Vec<strangecoin_core::types::BlockHeader> {
        Vec::new()
    }

    fn blocks_by_hashes(&self, _hashes: &[[u8; 32]], _max: usize) -> Vec<Block> {
        Vec::new()
    }
}

// ------------------------------------------------------------ tx/block builders

/// Regtest (`chain_id = 3`) transfer — every Stage 2 integration test runs
/// on regtest (prompt S2-P22 КГ).
pub fn transfer_tx(nonce: u64) -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "rsc1sender".into(),
        to: "rsc1receiver".into(),
        amount: 10,
        fee: 0,
        nonce,
        chain_id: 3,
        signature: vec![1u8; 65],
        is_coinbase: false,
    })
}

/// Regtest coinbase (one per block).
pub fn coinbase_tx() -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "rsc1miner".into(),
        to: "rsc1miner".into(),
        amount: 50,
        fee: 0,
        nonce: 0,
        chain_id: 3,
        signature: vec![2u8; 65],
        is_coinbase: true,
    })
}

/// Create tx with `code_len` bytes of init code — used to build a
/// realistically large (~1 MB) block for the bandwidth measurement.
pub fn padded_create_tx(nonce: u64, code_len: usize) -> Transaction {
    Transaction::Create(CreateTx {
        from: "rsc1deployer".into(),
        init_code: vec![0x00u8; code_len],
        init_args: Vec::new(),
        constructor_params: Vec::new(),
        endowment: 0,
        gas_limit: 1_000_000,
        fee: 1,
        nonce,
        chain_id: 3,
        signature: vec![3u8; 65],
    })
}

/// Build a self-consistent block: canonical `tx_root` + header hash.
pub fn make_block(txs: Vec<Transaction>) -> Block {
    let mut block = Block {
        index: 9,
        timestamp: 1_700_000_009,
        transactions: txs,
        previous_hash: "ee".repeat(32),
        hash: String::new(),
        nonce: 44,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [7u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    let ids: Vec<[u8; 32]> = block.transactions.iter().map(txid).collect();
    block.tx_root = merkle_root(&ids);
    block.hash = hex::encode(block_hash(&block));
    block
}

// ------------------------------------------------------------ noise helpers

/// `node_id` for a raw identity seed.
pub fn node_id_of(seed: [u8; 32]) -> NodeId {
    NodeKeypair::from_seed(seed).node_id()
}

/// A finished in-memory Noise XX handshake: both transports plus the
/// identities each side authenticates. No sockets — pure state machine.
pub fn transport_pair(
    identity_a: [u8; 32],
    identity_b: [u8; 32],
) -> (NoiseTransport, NoiseTransport, NodeId, NodeId) {
    let key_a = NodeKeypair::from_seed(identity_a);
    let key_b = NodeKeypair::from_seed(identity_b);
    let mut initiator = NoiseHandshake::initiator(
        &key_a.noise_static_private_key(),
        &key_a,
        Some(key_b.node_id()),
    )
    .expect("initiator builds");
    let mut responder = NoiseHandshake::responder(&key_b.noise_static_private_key(), &key_b, None)
        .expect("responder builds");
    let mut msg = [0u8; HANDSHAKE_BUF];
    let mut payload = [0u8; HANDSHAKE_BUF];

    let n = initiator.write_message(&[], &mut msg).expect("round 1");
    responder
        .read_message(&msg[..n], &mut payload)
        .expect("round 1 read");
    let n = responder.write_message(&[], &mut msg).expect("round 2");
    initiator
        .read_message(&msg[..n], &mut payload)
        .expect("round 2 read");
    let n = initiator.write_message(&[], &mut msg).expect("round 3");
    responder
        .read_message(&msg[..n], &mut payload)
        .expect("round 3 read");

    let (transport_a, peer_a) = initiator.into_transport().expect("initiator transport");
    let (transport_b, peer_b) = responder.into_transport().expect("responder transport");
    (transport_a, transport_b, peer_a, peer_b)
}

// ----------------------------------------------------------- peer manager

/// Deterministic in-memory `PeerManager` for integration tests: regtest
/// network id, optional `network_json` persistence path (restart tests use
/// a [`strangecoin::test_support::TestDir`] file).
pub fn manager_with(network_json: Option<PathBuf>) -> PeerManager {
    let config = PeerManagerConfig {
        network_id: 3,
        network_json_path: network_json,
        ..PeerManagerConfig::default()
    };
    PeerManager::new(config, NodeKeypair::from_seed([0xC1; 32]))
}
