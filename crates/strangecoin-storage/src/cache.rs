//! `CachedEngine` — capacity-bounded LRU cache over block/tx reads
//! (S3-P10, ADR-0024 block-read cache).
//!
//! # What is cached
//!
//! Only **[`Cf::Blocks`] and [`Cf::Transactions`]** point reads through
//! [`StorageEngine::get`]. Those are the hot gossip/sync paths
//! (`block_by_hash`, `tx_location`, `get_tx`, `chain_contains_*` — S3-P09
//! indices resolve to a second point-get on these CFs).
//!
//! **[`Cf::State`] is deliberately never cached here**: the current
//! consensus state already lives in the monolith's in-memory `StateCache`
//! (full state in RAM — required for the state root, ARCHITECT3 §3.4);
//! a second copy would only risk divergence. `state_history`, `meta`,
//! `contract_storage` and `mempool` are low-cardinality or write-once
//! paths where a cache adds invalidation surface for no measurable win.
//!
//! # Consistency
//!
//! Every `put`/`delete`/`write_batch` that touches a cached CF
//! **invalidates** the affected keys after the inner write succeeds — a
//! reorg that deletes block keys (S3-P09 Phase A) therefore cannot serve
//! a stale hit. `iterate_prefix` bypasses the cache entirely (it neither
//! reads nor populates): scans are rare (load/migration) and caching a
//! prefix sweep would blur capacity accounting.
//!
//! *Concurrency note*: invalidation happens after the inner write
//! returns. A concurrent `get` that missed before the write could, in
//! principle, re-insert the pre-write value in the window before
//! invalidation runs. The monolith excludes that window by holding the
//! `Blockchain` RwLock across engine calls (ADR-0024 §Concurrency —
//! reads take the read guard, adopts/reorgs take the write guard), so
//! this wrapper does not add its own cross-key locking beyond the cache
//! mutex itself.
//!
//! # Determinism (Stage 3 checklist)
//!
//! No clock, no RNG: hit/miss/eviction are pure functions of the key
//! sequence and `capacity`. `capacity == 0` disables the cache (every
//! call passes straight through) — the config's "off" switch.

use std::num::NonZeroUsize;
use std::sync::Mutex;

use lru::LruCache;

use crate::engine::{Cf, KvPair, StorageEngine, StorageError, WriteBatchOp};

/// CFs whose point reads are cacheable (see module docs).
fn is_cached_cf(cf: Cf) -> bool {
    matches!(cf, Cf::Blocks | Cf::Transactions)
}

/// One LRU slot: `((cf ordinal, raw key) → cached value bytes)`.
/// Aliased so the `cache` field stays readable (clippy
/// `type_complexity`); the type itself is unchanged.
type CacheMap = LruCache<(usize, Vec<u8>), Vec<u8>>;

/// StorageEngine decorator: LRU cache in front of `inner` for
/// [`Cf::Blocks`] / [`Cf::Transactions`] point reads.
pub struct CachedEngine<E: StorageEngine> {
    inner: E,
    /// Keyed by `(cf.ordinal(), key)` so the two CFs share one
    /// capacity budget without colliding. `Mutex` (not `RwLock`) —
    /// `LruCache::get` mutates recency on hit.
    ///
    /// `None` ⇔ requested capacity was 0 (config "off" switch): every
    /// call passes through and nothing is ever inserted.
    cache: Option<Mutex<CacheMap>>,
}

impl<E: StorageEngine> CachedEngine<E> {
    /// Wrap `inner` with a cache of at most `capacity` entries.
    ///
    /// `capacity == 0` → cache disabled (pure pass-through; the config
    /// layer may also skip wrapping entirely — both are equivalent).
    pub fn new(inner: E, capacity: usize) -> Self {
        let cache = NonZeroUsize::new(capacity).map(|cap| Mutex::new(LruCache::new(cap)));
        Self { inner, cache }
    }

    /// The configured entry capacity (0 = disabled).
    #[cfg(test)]
    fn capacity(&self) -> usize {
        match &self.cache {
            None => 0,
            Some(cache) => cache.lock().expect("cached engine lock poisoned").cap().get(),
        }
    }
}

impl<E: StorageEngine> StorageEngine for CachedEngine<E> {
    fn get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        if !is_cached_cf(cf) {
            return self.inner.get(cf, key);
        }
        let k = (cf.ordinal(), key.to_vec());
        if let Some(cache) = &self.cache {
            {
                let mut cache = cache.lock().expect("cached engine lock poisoned");
                if let Some(hit) = cache.get(&k) {
                    return Ok(Some(hit.clone()));
                }
            }
            let value = self.inner.get(cf, key)?;
            if let Some(v) = &value {
                let mut cache = cache.lock().expect("cached engine lock poisoned");
                // Overwrite with the value we just read from `inner` —
                // at least as fresh as any concurrent insert.
                cache.put(k, v.clone());
            }
            return Ok(value);
        }
        // capacity == 0: disabled, pure pass-through.
        self.inner.get(cf, key)
    }

    fn write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError> {
        // Collect invalidation keys before the ops are consumed.
        let stale: Vec<(Cf, Vec<u8>)> = ops
            .iter()
            .filter(|op| {
                let cf = match op {
                    WriteBatchOp::Put { cf, .. } | WriteBatchOp::Delete { cf, .. } => *cf,
                };
                is_cached_cf(cf)
            })
            .map(|op| match op {
                WriteBatchOp::Put { cf, key, .. } | WriteBatchOp::Delete { cf, key } => {
                    (*cf, key.clone())
                }
            })
            .collect();

        self.inner.write_batch(ops)?;

        // Invalidate only after the write is visible — a failed batch
        // leaves the cache untouched (the old value is still correct).
        if let Some(cache) = &self.cache {
            let mut cache = cache.lock().expect("cached engine lock poisoned");
            for (cf, key) in stale {
                cache.pop(&(cf.ordinal(), key));
            }
        }
        Ok(())
    }

    fn iterate_prefix(&self, cf: Cf, prefix: &[u8]) -> Result<Vec<KvPair>, StorageError> {
        // Bypass: scans neither hit nor populate (module docs).
        self.inner.iterate_prefix(cf, prefix)
    }

    fn flush(&self) -> Result<(), StorageError> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::WriteBatchOp;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Minimal sorted engine for cache-behaviour tests (same shape as
    /// the MemoryEngine in `engine.rs`, duplicated here to keep the
    /// cache tests self-contained).
    struct MapEngine {
        cfs: Mutex<[BTreeMap<Vec<u8>, Vec<u8>>; Cf::ALL.len()]>,
    }

    impl MapEngine {
        fn new() -> Self {
            Self {
                cfs: Mutex::new(std::array::from_fn(|_| BTreeMap::new())),
            }
        }
    }

    impl StorageEngine for MapEngine {
        fn get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
            let cfs = self.cfs.lock().unwrap();
            Ok(cfs[cf.ordinal()].get(key).cloned())
        }

        fn write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError> {
            let mut cfs = self.cfs.lock().unwrap();
            for op in ops {
                match op {
                    WriteBatchOp::Put { cf, key, value } => {
                        cfs[cf.ordinal()].insert(key, value);
                    }
                    WriteBatchOp::Delete { cf, key } => {
                        cfs[cf.ordinal()].remove(&key);
                    }
                }
            }
            Ok(())
        }

        fn iterate_prefix(&self, cf: Cf, prefix: &[u8]) -> Result<Vec<KvPair>, StorageError> {
            let cfs = self.cfs.lock().unwrap();
            Ok(cfs[cf.ordinal()]
                .range(prefix.to_vec()..)
                .take_while(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect())
        }

        fn flush(&self) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[test]
    fn hit_returns_cached_value_and_survives_inner_mutation_without_invalidation() {
        let cached = CachedEngine::new(MapEngine::new(), 8);
        cached.put(Cf::Blocks, b"h1", b"v1").unwrap();

        // First get populates; second get is a pure cache hit.
        assert_eq!(cached.get(Cf::Blocks, b"h1").unwrap(), Some(b"v1".to_vec()));
        assert_eq!(cached.get(Cf::Blocks, b"h1").unwrap(), Some(b"v1".to_vec()));

        // Direct inner mutation bypassing the decorator must not be
        // visible through the cache (documents WHY invalidation lives
        // in write_batch — a caller that mutates `inner` directly would
        // go stale; the trait only exposes the decorator).
        cached.inner.put(Cf::Blocks, b"h1", b"stale").unwrap();
        assert_eq!(
            cached.get(Cf::Blocks, b"h1").unwrap(),
            Some(b"v1".to_vec()),
            "cache serves the value it holds until invalidated"
        );
    }

    #[test]
    fn write_batch_invalidates_affected_keys_only() {
        let cached = CachedEngine::new(MapEngine::new(), 8);
        cached.put(Cf::Blocks, b"a", b"old-a").unwrap();
        cached.put(Cf::Blocks, b"b", b"old-b").unwrap();
        cached.put(Cf::Transactions, b"t", b"old-t").unwrap();
        // Warm the hits.
        let _ = cached.get(Cf::Blocks, b"a").unwrap();
        let _ = cached.get(Cf::Blocks, b"b").unwrap();
        let _ = cached.get(Cf::Transactions, b"t").unwrap();

        cached
            .write_batch(vec![WriteBatchOp::Put {
                cf: Cf::Blocks,
                key: b"a".to_vec(),
                value: b"new-a".to_vec(),
            }])
            .unwrap();

        assert_eq!(
            cached.get(Cf::Blocks, b"a").unwrap(),
            Some(b"new-a".to_vec()),
            "invalidated key must re-read from inner"
        );
        assert_eq!(
            cached.get(Cf::Blocks, b"a").unwrap(),
            Some(b"new-a".to_vec()),
            "and re-populate the cache"
        );
        assert_eq!(
            cached.get(Cf::Blocks, b"b").unwrap(),
            Some(b"old-b".to_vec()),
            "untouched key stays cached"
        );
        assert_eq!(
            cached.get(Cf::Transactions, b"t").unwrap(),
            Some(b"old-t".to_vec()),
            "other cached CF untouched"
        );
    }

    #[test]
    fn delete_invalidates_and_absent_reads_do_not_populate() {
        let cached = CachedEngine::new(MapEngine::new(), 8);
        cached.put(Cf::Blocks, b"gone", b"v").unwrap();
        let _ = cached.get(Cf::Blocks, b"gone").unwrap();

        cached.delete(Cf::Blocks, b"gone").unwrap();
        assert_eq!(cached.get(Cf::Blocks, b"gone").unwrap(), None);

        // A miss on an absent key must not insert None-shaped junk:
        // subsequent inner insert (via a path that invalidates) wins.
        assert_eq!(cached.get(Cf::Blocks, b"never").unwrap(), None);
        cached
            .put(Cf::Blocks, b"never", b"now-here")
            .unwrap();
        assert_eq!(
            cached.get(Cf::Blocks, b"never").unwrap(),
            Some(b"now-here".to_vec())
        );
    }

    #[test]
    fn capacity_zero_never_populates() {
        let cached = CachedEngine::new(MapEngine::new(), 0);
        assert_eq!(cached.capacity(), 0);
        cached.put(Cf::Blocks, b"k", b"v").unwrap();
        assert_eq!(cached.get(Cf::Blocks, b"k").unwrap(), Some(b"v".to_vec()));

        // Disabled cache = pure pass-through: a direct inner mutation
        // is visible immediately (nothing was retained).
        cached.inner.put(Cf::Blocks, b"k", b"v2").unwrap();
        assert_eq!(
            cached.get(Cf::Blocks, b"k").unwrap(),
            Some(b"v2".to_vec()),
            "capacity 0 must never retain entries"
        );
    }

    #[test]
    fn lru_evicts_least_recently_used_when_over_capacity() {
        // Design note: `put` invalidates (write-through-invalidate), it
        // does not populate — entries enter the cache only via a `get`
        // miss that finds a value in `inner`. The sequence below warms
        // two entries, refreshes `a` to MRU, then admits `c`.
        let cached = CachedEngine::new(MapEngine::new(), 2);

        cached.put(Cf::Blocks, b"a", b"va").unwrap();
        cached.put(Cf::Blocks, b"b", b"vb").unwrap();
        let _ = cached.get(Cf::Blocks, b"a").unwrap(); // warm a (MRU after)
        let _ = cached.get(Cf::Blocks, b"b").unwrap(); // warm b
        let _ = cached.get(Cf::Blocks, b"a").unwrap(); // refresh a → MRU

        // Admit c: capacity 2, LRU is b → b is evicted, a stays.
        cached.put(Cf::Blocks, b"c", b"vc").unwrap();
        let _ = cached.get(Cf::Blocks, b"c").unwrap();

        // Prove residency by mutating `inner` directly: a cached key
        // keeps serving the old bytes, an evicted key re-reads.
        // Order matters: a cache miss on `b` re-inserts and would evict
        // another entry — assert the still-resident keys FIRST.
        cached.inner.put(Cf::Blocks, b"a", b"va2").unwrap();
        cached.inner.put(Cf::Blocks, b"b", b"vb2").unwrap();
        cached.inner.put(Cf::Blocks, b"c", b"vc2").unwrap();

        assert_eq!(
            cached.get(Cf::Blocks, b"a").unwrap(),
            Some(b"va".to_vec()),
            "a was MRU → still cached"
        );
        assert_eq!(
            cached.get(Cf::Blocks, b"c").unwrap(),
            Some(b"vc".to_vec()),
            "c is the newest entry → still cached"
        );
        assert_eq!(
            cached.get(Cf::Blocks, b"b").unwrap(),
            Some(b"vb2".to_vec()),
            "b was LRU → evicted → miss → inner value"
        );
    }

    #[test]
    fn state_cf_is_never_cached() {
        let cached = CachedEngine::new(MapEngine::new(), 8);
        cached.put(Cf::State, b"addr", b"v1").unwrap();
        // Warm a get (would populate if State were cacheable).
        assert_eq!(cached.get(Cf::State, b"addr").unwrap(), Some(b"v1".to_vec()));

        // Direct inner mutation is immediately visible → State bypasses.
        cached.inner.put(Cf::State, b"addr", b"v2").unwrap();
        assert_eq!(
            cached.get(Cf::State, b"addr").unwrap(),
            Some(b"v2".to_vec()),
            "state CF must pass through (StateCache owns the real copy)"
        );
    }

    #[test]
    fn iterate_prefix_bypasses_cache_in_both_directions() {
        let cached = CachedEngine::new(MapEngine::new(), 8);
        cached.put(Cf::Blocks, b"p1", b"x").unwrap();
        cached.put(Cf::Blocks, b"p2", b"y").unwrap();

        // Scan sees inner truth even for keys the cache holds.
        cached.inner.put(Cf::Blocks, b"p1", b"x-inner").unwrap();
        let entries = cached.iterate_prefix(Cf::Blocks, b"p").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, b"x-inner", "scan reads inner, not cache");

        // Scan does not populate: a subsequent get re-reads inner.
        assert_eq!(
            cached.get(Cf::Blocks, b"p1").unwrap(),
            Some(b"x-inner".to_vec())
        );
    }
}
