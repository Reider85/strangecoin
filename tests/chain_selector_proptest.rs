use proptest::prelude::*;
use strangecoin::blockchain::chain_selector::{ChainInfo, ChainSelector};
use strangecoin_core::consensus::u256_gt;

fn arb_u256() -> impl Strategy<Value = strangecoin_core::consensus::U256> {
    (0u64..1000, 0u64..1000, 0u64..1000, 0u64..1000).prop_map(|(a, b, c, d)| [a, b, c, d])
}

fn arb_chain_info() -> impl Strategy<Value = ChainInfo> {
    (
        0u64..1000u64,
        "[a-f0-9]{4,8}",
        arb_u256(),
        0u64..1_000_000u64,
    )
        .prop_map(|(tip_height, tip_hash, total_work, tip_timestamp)| ChainInfo {
            tip_height,
            tip_hash,
            total_work,
            tip_timestamp,
        })
}

proptest! {
    #[test]
    fn select_best_permutation_invariant(mut chains in prop::collection::vec(arb_chain_info(), 1..5)) {
        let expected_hash = {
            let refs: Vec<&ChainInfo> = chains.iter().collect();
            ChainSelector::select_best(&refs).unwrap().tip_hash.clone()
        };

        // Shuffle and verify same result
        chains.reverse();
        let reversed_refs: Vec<&ChainInfo> = chains.iter().collect();
        let result_hash = ChainSelector::select_best(&reversed_refs).unwrap().tip_hash.clone();

        prop_assert_eq!(expected_hash, result_hash);
    }

    #[test]
    fn transitivity(x in 0u64..1000, step1 in 1u64..1000, step2 in 1u64..1000) {
        // Strictly increasing work is built in, so no input filtering is needed.
        let w1 = [0, 0, 0, x];
        let w2 = [0, 0, 0, x + step1];
        let w3 = [0, 0, 0, x + step1 + step2];
        let base = |work, hash: &str| ChainInfo {
            tip_height: 5,
            tip_hash: hash.to_string(),
            total_work: work,
            tip_timestamp: 1000,
        };
        let a = base(w1, "aaa");
        let b = base(w2, "aaa");
        let c = base(w3, "aaa");
        prop_assert!(ChainSelector::is_better(&b, &a));
        prop_assert!(ChainSelector::is_better(&c, &b));
        prop_assert!(ChainSelector::is_better(&c, &a));
    }

    #[test]
    fn work_dominates_timestamp(
        high_work_low_ts in arb_chain_info(),
        low_work_high_ts in arb_chain_info(),
    ) {
        prop_assume!(u256_gt(high_work_low_ts.total_work, low_work_high_ts.total_work));
        prop_assume!(high_work_low_ts.tip_timestamp > low_work_high_ts.tip_timestamp);
        prop_assert!(ChainSelector::is_better(&high_work_low_ts, &low_work_high_ts));
    }

    #[test]
    fn timestamp_dominates_hash(
        total_work in arb_u256(),
        earlier_ts in 0u64..1_000_000u64,
        later_ts in 0u64..1_000_000u64,
        high_hash in "[a-f0-9]{4,8}",
        low_hash in "[a-f0-9]{4,8}",
    ) {
        prop_assume!(earlier_ts < later_ts);
        prop_assume!(low_hash < high_hash);
        let earlier = ChainInfo {
            tip_height: 5,
            tip_hash: high_hash,
            total_work,
            tip_timestamp: earlier_ts,
        };
        let later = ChainInfo {
            tip_height: 5,
            tip_hash: low_hash,
            total_work,
            tip_timestamp: later_ts,
        };
        prop_assert!(ChainSelector::is_better(&earlier, &later));
    }

    #[test]
    fn hash_tiebreak(
        total_work in arb_u256(),
        tip_timestamp in 0u64..1_000_000u64,
        hash_a in "[a-f0-9]{4,8}",
        hash_b in "[a-f0-9]{4,8}",
    ) {
        prop_assume!(hash_a != hash_b);
        let build = |hash: String| ChainInfo {
            tip_height: 5,
            tip_hash: hash,
            total_work,
            tip_timestamp,
        };
        let a = build(hash_a);
        let b = build(hash_b);
        if a.tip_hash < b.tip_hash {
            prop_assert!(ChainSelector::is_better(&a, &b));
        } else {
            prop_assert!(ChainSelector::is_better(&b, &a));
        }
    }

    #[test]
    fn select_best_returns_something(chains in prop::collection::vec(arb_chain_info(), 1..10)) {
        let refs: Vec<&ChainInfo> = chains.iter().collect();
        prop_assert!(ChainSelector::select_best(&refs).is_some());
    }

    #[test]
    fn no_chain_means_none(chains in prop::collection::vec(arb_chain_info(), 0..1)) {
        let refs: Vec<&ChainInfo> = chains.iter().collect();
        if refs.is_empty() {
            prop_assert!(ChainSelector::select_best(&refs).is_none());
        }
    }
}
