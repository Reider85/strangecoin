---- MODULE consensus ----
EXTENDS Naturals, Sequences

\* Bounded model of Strangecoin consensus safety rules.
\* Scaled-down constants make the full property set TLC-checkable
\* (BUG-S0-026): 2 addresses, <= 3 blocks, <= 1 transfer per block.
\* Emission formula mirrors crates/strangecoin-core/src/economics/emission.rs:
\*   reward = max(base_reward(height), tail_reward(total_supply))
\* with tail_rate = TailRateNum/(TailRateDen*BlocksPerYear) chosen so the
\* tail branch is reachable inside the bounded chain (non-vacuous NoInflation).

CONSTANTS
    ChainId,
    AddrSet,
    MaxBlocks,
    MaxAmount,
    MaxNonce,
    InitialBalance,
    InitialReward,
    HalvingInterval,
    TailRateNum,
    TailRateDen,
    BlocksPerYear,
    MaxTime,
    MaxHash,
    MaxTarget

\* ---------- Types ----------

Tx == [
    sender: AddrSet,
    receiver: AddrSet,
    amount: 1..MaxAmount,
    nonce: 1..MaxNonce,
    chain_id: {ChainId},
    signature: {1}          \* abstract: nonzero = "signed" (crypto out of scope)
]

\* At most one transfer per block (bounded); <<>> = no transfer (coinbase only)
TxBatches == {<<>>} \cup {<<t>> : t \in Tx}

Block == [
    index: 1..MaxBlocks,
    timestamp: 0..MaxTime,
    txs: TxBatches,
    prev_hash: 0..MaxHash,
    hash: 0..MaxHash,
    target: 1..MaxTarget,
    coinbase_amount: 0..(InitialReward * MaxBlocks),
    chain_id: {ChainId}
]

VARIABLES chain, balances, nonces, time, supply

vars == <<chain, balances, nonces, time, supply>>

\* ---------- Emission schedule (scaled mirror of emission.rs) ----------

RewardAtHeight(height, tot_supply) ==
    LET halvings == height \div HalvingInterval IN
    LET base_reward ==
        IF halvings >= 2 THEN 0 ELSE InitialReward \div (2 ^ halvings)
    IN
    LET tail_reward == (tot_supply * TailRateNum) \div (TailRateDen * BlocksPerYear) IN
    IF base_reward >= tail_reward THEN base_reward ELSE tail_reward

\* ---------- Invariants ----------

TypeInvariant ==
    /\ chain \in Seq(Block)
    /\ balances \in [AddrSet -> Nat]
    /\ nonces \in [AddrSet -> Nat]
    /\ time \in Nat
    /\ supply \in Nat

\* Total supply before block i = sum of coinbases of blocks 1..i-1
\* (depth bounded by Len(chain) <= MaxBlocks)
RECURSIVE SupplyBefore(_, _)
SupplyBefore(ch, i) ==
    IF i <= 1 THEN 0
    ELSE ch[i-1].coinbase_amount + SupplyBefore(ch, i-1)

\* Tracked supply variable equals the sum of all coinbases minted so far
SupplyConsistency ==
    supply = SupplyBefore(chain, Len(chain) + 1)

\* No two transfers from the same sender reuse a nonce anywhere in the chain
NoDoubleSpend ==
    \A i \in DOMAIN chain:
        \A j \in DOMAIN chain:
            i < j =>
                \A ti \in {chain[i].txs[k] : k \in DOMAIN chain[i].txs}:
                    \A tj \in {chain[j].txs[k] : k \in DOMAIN chain[j].txs}:
                        ~(ti.sender = tj.sender /\ ti.nonce = tj.nonce)

\* Every block mints exactly the emission schedule reward for its height/supply
NoInflation ==
    \A i \in DOMAIN chain:
        chain[i].coinbase_amount =
            RewardAtHeight(chain[i].index, SupplyBefore(chain, i))

\* All transfers are signed (abstract nonzero signature)
AllTxSigned ==
    \A i \in DOMAIN chain:
        \A k \in DOMAIN chain[i].txs:
            chain[i].txs[k].signature # 0

\* Nonces of one sender strictly increase along the chain
NonceMonotonic ==
    \A i \in DOMAIN chain:
        \A j \in DOMAIN chain:
            i < j =>
                \A ti \in {chain[i].txs[k] : k \in DOMAIN chain[i].txs}:
                    \A tj \in {chain[j].txs[k] : k \in DOMAIN chain[j].txs}:
                        ti.sender = tj.sender => ti.nonce < tj.nonce

ChainContinuity ==
    \A i \in 2..Len(chain):
        chain[i].prev_hash = chain[i - 1].hash

PowValidity ==
    \A i \in DOMAIN chain:
        chain[i].hash <= chain[i].target

ChainIdConsistency ==
    /\ \A i \in DOMAIN chain:
        chain[i].chain_id = ChainId
    /\ \A i \in DOMAIN chain:
        \A k \in DOMAIN chain[i].txs:
            chain[i].txs[k].chain_id = ChainId

\* ---------- State transitions ----------

ApplicableTx(tx) ==
    /\ tx.chain_id = ChainId
    /\ tx.signature # 0
    /\ tx.nonce = nonces[tx.sender] + 1
    /\ tx.amount <= balances[tx.sender]
    /\ tx.sender # tx.receiver

Init ==
    /\ chain = <<>>
    /\ balances = [a \in AddrSet |-> InitialBalance]
    /\ nonces = [a \in AddrSet |-> 0]
    /\ time = 0
    /\ supply = 0

AddBlock ==
    LET new_index == Len(chain) + 1 IN
    LET reward == RewardAtHeight(new_index, supply) IN
    /\ new_index <= MaxBlocks
    /\ \E txs \in TxBatches, hash_val \in 0..MaxHash, target_val \in 1..MaxTarget:
        LET new_block == [
            index |-> new_index,
            timestamp |-> time,
            txs |-> txs,
            prev_hash |-> IF new_index = 1 THEN 0 ELSE chain[Len(chain)].hash,
            hash |-> hash_val,
            target |-> target_val,
            coinbase_amount |-> reward,
            chain_id |-> ChainId
        ] IN
        /\ hash_val <= target_val
        /\ \A k \in DOMAIN txs: ApplicableTx(txs[k])
        /\ chain' = Append(chain, new_block)
        /\ balances' =
            IF txs = <<>> THEN balances
            ELSE [balances EXCEPT
                    ![txs[1].sender] = balances[txs[1].sender] - txs[1].amount,
                    ![txs[1].receiver] = balances[txs[1].receiver] + txs[1].amount]
        /\ nonces' =
            IF txs = <<>> THEN nonces
            ELSE [nonces EXCEPT ![txs[1].sender] = txs[1].nonce]
        /\ supply' = supply + reward
        /\ time' = time

AdvanceTime ==
    /\ time < MaxTime
    /\ time' = time + 1
    /\ UNCHANGED <<chain, balances, nonces, supply>>

Next == AddBlock \/ AdvanceTime

\* State space is finite by construction: AddBlock is guarded by
\* new_index <= MaxBlocks, AdvanceTime by time < MaxTime. (No state
\* CONSTRAINT: combining constraints with liveness checking is unsound
\* per Specifying Systems §14.3.5 — TLC even warns about it.)

\* AddBlock is continuously enabled until the chain is full; weak fairness
\* on AddBlock (mining) forces eventual chain growth. AdvanceTime is
\* bounded and needs no fairness. IMPORTANT: the .cfg must use
\* SPECIFICATION Spec — with INIT/NEXT TLC ignores fairness conjuncts.
Spec == Init /\ [][Next]_vars /\ WF_vars(AddBlock)

Liveness == <>[](Len(chain) = MaxBlocks)

THEOREM Spec => [](
    /\ TypeInvariant
    /\ SupplyConsistency
    /\ NoDoubleSpend
    /\ NoInflation
    /\ AllTxSigned
    /\ NonceMonotonic
    /\ ChainContinuity
    /\ PowValidity
    /\ ChainIdConsistency
)

=============================================================================
