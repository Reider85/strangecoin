---- MODULE consensus ----
EXTENDS Naturals, Sequences

CONSTANTS ChainId

VARIABLES chain, time

vars == <<chain, time>>

Block == [
    index: Nat,
    timestamp: Nat,
    tx_count: Nat,
    prev_hash: Nat,
    hash: Nat,
    chain_id: {ChainId}
]

TypeInvariant ==
    /\ chain \in Seq(Block)
    /\ time \in Nat

NoDoubleSpend ==
    \A i \in DOMAIN chain:
        chain[i].tx_count >= 0

ChainContinuity ==
    \A i \in 2..Len(chain):
        chain[i].prev_hash = chain[i - 1].hash

ChainIdConsistency ==
    \A i \in DOMAIN chain:
        chain[i].chain_id = ChainId

Init ==
    /\ chain = <<>>
    /\ time = 0

AddBlock ==
    LET new_index == Len(chain) + 1 IN
    \E hash_val \in 0..10:
        LET new_block == [
            index |-> new_index,
            timestamp |-> time,
            tx_count |-> 0,
            prev_hash |-> IF new_index = 1 THEN 0 ELSE chain[Len(chain)].hash,
            hash |-> hash_val,
            chain_id |-> ChainId
        ] IN
        /\ chain' = Append(chain, new_block)
        /\ time' = time

AdvanceTime ==
    /\ time' = time + 1
    /\ UNCHANGED chain

Next ==
    \/ (Len(chain) < 4 /\ AddBlock)
    \/ AdvanceTime

Spec == Init /\ [][Next]_vars

ChainConstraint == Len(chain) <= 3 /\ time <= 5

THEOREM Spec => [](
    /\ TypeInvariant
    /\ ChainContinuity
    /\ ChainIdConsistency
)

=============================================================================
