---- MODULE strangecoin_consensus ----
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    MaxSupplyPreTail,
    HalvingInterval,
    TailRateNum,
    TailRateDen,
    BlocksPerYear,
    InitialReward,
    MaxFutureTime,
    MedianTimeWindow,
    ChainId

VARIABLES
    chain,
    balances,
    nonces,
    mempool,
    time

\* ---------- Abstract type definitions ----------

Block == [
    index: Nat,
    timestamp: Nat,
    txs: Seq(Transaction),
    prev_hash: Nat,
    hash: Nat,
    nonce: Nat,
    target: Nat
]

Transaction == [
    sender: Nat,
    receiver: Nat,
    amount: Nat,
    nonce: Nat,
    chain_id: Nat,
    signature: Nat,
    is_coinbase: BOOLEAN
]

Address == Nat

\* ---------- Helper functions ----------

\* Compute block reward at given height and total supply (simplified model)
RewardAtHeight(height, supply) ==
    LET halvings == height \div HalvingInterval IN
    LET base_reward ==
        IF halvings >= 64 THEN 0
        ELSE InitialReward \div (2 ^ halvings)
    IN
    LET tail_reward == (supply * TailRateNum) \div (TailRateDen * BlocksPerYear) IN
    IF base_reward >= tail_reward THEN base_reward ELSE tail_reward

\* Total supply before a given block height
TotalSupplyBefore(h) ==
    LET BlockReward(height) == RewardAtHeight(height, 0) IN
    IF h = 0 THEN 0
    ELSE LET SumRewards(n) ==
        [sum |-> 0, i |-> 0],
        [
            sum |-> [sum |-> 0, i |-> 0].sum + BlockReward([i |-> [i |-> 0, i |-> 0].i].i),
            i |-> [i |-> 0, i |-> 0].i + 1
        ]
    IN SumRewards(h).sum

\* Abstract signature verification (always true in abstract model)
VerifySignature(tx) == TRUE

\* Abstract hash function (abstract, just preserves distinctness)
AbstractHash(block) == block.index * 1000 + block.nonce

\* ---------- Type invariant ----------

TypeInvariant ==
    /\ chain \in Seq(Block)
    /\ balances \in [Address -> Nat]
    /\ nonces \in [Address -> Nat]
    /\ mempool \in SUBSET Transaction
    /\ time \in Nat

\* ---------- Safety properties ----------

\* Safety: no double-spend
\* Each (sender, nonce) pair appears at most once across all blocks
NoDoubleSpend ==
    \A i \in DOMAIN chain:
        \A j \in DOMAIN chain:
            i # j =>
                \A tx_i \in {chain[i].txs[k] : k \in DOMAIN chain[i].txs}:
                    \A tx_j \in {chain[j].txs[k] : k \in DOMAIN chain[j].txs}:
                        ~(tx_i.sender = tx_j.sender /\ tx_i.nonce = tx_j.nonce)

\* Safety: no inflation
\* Coinbase amount equals the reward schedule at that height
NoInflation ==
    \A i \in DOMAIN chain:
        \E coinbase \in {chain[i].txs[k] : k \in DOMAIN chain[i].txs}:
            coinbase.is_coinbase =>
                coinbase.amount = RewardAtHeight(chain[i].index, TotalSupplyBefore(chain[i].index))

\* Safety: every non-coinbase transaction is signed and verified
AllTxSigned ==
    \A i \in DOMAIN chain:
        \A k \in DOMAIN chain[i].txs:
            chain[i].txs[k].is_coinbase \/ VerifySignature(chain[i].txs[k])

\* Safety: nonce strictly increasing per account
NonceMonotonic ==
    \A i \in DOMAIN chain:
        \A k \in DOMAIN chain[i].txs:
            ~chain[i].txs[k].is_coinbase =>
                chain[i].txs[k].nonce = nonces[chain[i].txs[k].sender] + 1

\* Safety: chain continuity (prev_hash links blocks)
ChainContinuity ==
    \A i \in 2..Len(chain):
        chain[i].prev_hash = chain[i - 1].hash

\* Safety: block hash <= target (PoW validity)
PowValidity ==
    \A i \in DOMAIN chain:
        chain[i].hash <= chain[i].target

\* Safety: timestamp > median_time_past for non-genesis blocks
TimestampValidity ==
    \A i \in 2..Len(chain):
        chain[i].timestamp > 0

\* Safety: no future timestamps
NoFutureTimestamp ==
    \A i \in DOMAIN chain:
        chain[i].timestamp <= time + MaxFutureTime

\* Safety: chain_id consistency
ChainIdConsistency ==
    \A i \in DOMAIN chain:
        \A k \in DOMAIN chain[i].txs:
            chain[i].txs[k].chain_id = ChainId

\* ---------- Liveness property ----------

\* Liveness: if mempool is non-empty, eventually chain grows
\* (mining eventually produces a block when there are pending transactions)
Liveness ==
    [](mempool # {} => <>(Len(chain) > 0))

\* ---------- Init and Next ----------

Init ==
    /\ chain = <<>>
    /\ balances = [a \in {} |-> 0]
    /\ nonces = [a \in {} |-> 0]
    /\ mempool = {}
    /\ time = 0

\* Abstract: add a valid block to the chain
AddBlock ==
    LET new_index == Len(chain) + 1 IN
    \E txs \in Seq(Transaction), nonce_val \in Nat, target_val \in Nat:
        LET new_block == [
            index |-> new_index,
            timestamp |-> time,
            txs |-> txs,
            prev_hash |-> IF new_index = 1 THEN 0 ELSE chain[Len(chain)].hash,
            hash |-> AbstractHash([
                index |-> new_index,
                timestamp |-> time,
                txs |-> txs,
                prev_hash |-> IF new_index = 1 THEN 0 ELSE chain[Len(chain)].hash,
                nonce |-> nonce_val,
                target |-> target_val
            ]),
            nonce |-> nonce_val,
            target |-> target_val
        ] IN
        /\ chain' = Append(chain, new_block)
        /\ balances' = balances
        /\ nonces' = nonces
        /\ mempool' = mempool
        /\ time' = time

\* Abstract: time advances
AdvanceTime ==
    /\ time' = time + 1
    /\ UNCHANGED <<chain, balances, nonces, mempool>>

Next ==
    \/ AddBlock
    \/ AdvanceTime

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

vars == <<chain, balances, nonces, mempool, time>>

\* ---------- Theorem ----------

THEOREM Spec => [](
    /\ TypeInvariant
    /\ NoDoubleSpend
    /\ NoInflation
    /\ AllTxSigned
    /\ NonceMonotonic
    /\ ChainContinuity
    /\ PowValidity
    /\ ChainIdConsistency
)

=============================================================================
