use proptest::prelude::*;
use strangecoin::blockchain::chain_selector::{ChainInfo, ChainSelector};

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
        let refs: Vec<&ChainInfo> = chains.iter().collect();
        let expected = ChainSelector::select_best(&refs).unwrap();

        // Shuffle and verify same result
        chains.reverse();
        let reversed_refs: Vec<&ChainInfo> = chains.iter().collect();
        let result = ChainSelector::select_best(&reversed_refs).unwrap();

        prop_assert_eq!(expected.tip_hash, result.tip_hash);
    }

    #[test]
    fn transitivity(a in arb_chain_info(), b in arb_chain_info(), c in arb_chain_info()) {
        prop_assume!(ChainSelector::is_better(&a, &b));
        prop_assume!(ChainSelector::is_better(&b, &c));
        prop_assert!(ChainSelector::is_better(&a, &c));
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
        earlier_high_hash in arb_chain_info(),
        later_low_hash in arb_chain_info(),
    ) {
        prop_assume!(earlier_high_hash.total_work == later_low_hash.total_work);
        prop_assume!(earlier_high_hash.tip_timestamp < later_low_hash.tip_timestamp);
        prop_assume!(earlier_high_hash.tip_hash > later_low_hash.tip_hash);
        prop_assert!(ChainSelector::is_better(&earlier_high_hash, &later_low_hash));
    }

    #[test]
    fn hash_tiebreak(a in arb_chain_info(), b in arb_chain_info()) {
        prop_assume!(a.total_work == b.total_work);
        prop_assume!(a.tip_timestamp == b.tip_timestamp);
        prop_assume!(a.tip_hash != b.tip_hash);
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
