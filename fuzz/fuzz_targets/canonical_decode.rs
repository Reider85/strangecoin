//! First fuzz target (S1-P19, security track «Stage 1+: Fuzzing»): the
//! canonical deserializers on arbitrary bytes.
//!
//! Contract: every input either decodes or returns a typed error — a panic
//! (index-out-of-bounds, overflow, unwrap) is a crash. Allocation is bounded
//! by the decoders themselves: they reject absurd frame lengths before
//! reserving capacity (invariant #7), and libFuzzer's rss limit guards the
//! process as a second line of defense.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = strangecoin_core::serialize::deserialize_header(data);
    let _ = strangecoin_core::serialize::deserialize_block(data);
    let _ = strangecoin_core::serialize::deserialize_transaction(data);
    let _ = strangecoin_core::serialize::deserialize_transaction_signed(data);
});
