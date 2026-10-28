//! S2-P16 — token-bucket rate limiting per peer (ADR-0023).
//!
//! Stage 1 shipped per-class sliding windows ([`MessageClass`] counts inside a
//! fixed 10 s window with an immediate 5-minute ban on the first overflow).
//! ADR-0023 replaces that wholesale with lazy-refill token buckets:
//!
//! * one **main bucket** per peer (burst + steady-state rate — every inbound
//!   wire message draws 1 token from it),
//! * **per-message-type sub-buckets** for the Stage 2 gossip hot paths
//!   (`INV`/`BLOCK`/`TX`, charged in parallel with the main bucket) plus the
//!   legacy sync sub-buckets (`GET_HEADERS`/`GET_BLOCKS`) that the Stage 1
//!   classes mapped onto,
//! * a **violation window**: a refused message = 1 violation; the counter
//!   decays after `ban_window` of quiet,
//! * **ban on threshold**: `ban_threshold` violations lead to a
//!   `ban_duration` fast-path ban (5 min default, `config.toml: rate_limit_*`).
//!
//! Exhaustion is fail-closed: the message is refused (dropped, never queued),
//! the caller decides whether to keep the connection (`Exceeded`) or close it
//! (`Ban`). The peer_manager slow path (ADR-0022 `protocol_violations`) is fed
//! through [`PeerManager::apply_rate_verdict`](crate::peer::PeerManager::apply_rate_verdict).
//!
//! **Determinism** (Stage 2 checklist): refill arithmetic is a pure function of
//! bucket state + injected `now` — no `SystemTime`, no RNG, no background
//! timers. Wall time enters only through the `_at` API edge (the
//! `tick_at`/`reconcile_at` precedent); plain wrappers read `Instant::now()`
//! once at the edge. Tests drive a simulated clock, so no `sleep` appears in
//! this module's test suite.
//!
//! Keying stays `SocketAddr` (ADR-0023: works pre-HELLO, before `node_id` is
//! known); node_id-keyed consequences ride on the peer_manager bans.
//!
//! S2-P16 additions beyond the prompt sketch (recorded as deviations in the
//! commit): `banned_until` + `ban_duration` fields (ADR "ban 5 min" is not
//! expressible with only `ban_threshold`/`ban_window`), separate legacy sync
//! buckets (ADR bucket table: `GET_HEADERS`/`GET_BLOCKS`; the prompt's
//! 10-variant [`MessageType`] has no sync variants), and clock-injected `_at`
//! variants.

use crate::error::NetError;
use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Wire-message type for Stage 2 gossip/protocol traffic (prompt S2-P16:
/// exactly 10 variants).
///
/// A sub-bucket exists only for the types listed in `rate_limit_per_type`
/// (`INV`/`BLOCK`/`TX` by default — ADR-0023); every other type draws from
/// the main bucket only (ADR: "HELLO, PING, ADDR, GET_DATA ... main bucket
/// only").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageType {
    /// `MSG_INV` inventory announce (ADR-0023: 100 burst / 10 per s).
    Inv,
    /// `MSG_BLOCKS` block announce (30 burst / 3 per s).
    Block,
    /// `MSG_TX` transaction announce (1000 burst / 100 per s).
    Tx,
    /// `HELLO` handshake — main bucket only.
    Hello,
    /// `MSG_GET_DATA` — main bucket only.
    GetData,
    /// `MSG_COMPACT_BLOCK` (BIP 152) — main bucket only.
    CompactBlock,
    /// `MSG_GET_BLOCK_TXN` — main bucket only.
    GetBlockTxn,
    /// `MSG_BLOCK_TXN` — main bucket only.
    BlockTxn,
    /// Erlay reconciliation envelope — main bucket only.
    ErlayReconcile,
    /// `MSG_ADDR` gossip — main bucket only.
    Addr,
}

/// Legacy Stage 1 classification. Kept as the type-to-sub-bucket mapping
/// until S2-P18 drops the legacy classes entirely (ADR-0023 "Neutral"):
/// `GetHeaders`/`GetBlocks` charge their dedicated sync sub-buckets,
/// `ChainGossip`/`Other` draw from the main bucket only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageClass {
    /// `MSG_GET_HEADERS` — sync sub-bucket (100 burst / 10 per s).
    GetHeaders,
    /// `MSG_GET_BLOCKS` — sync sub-bucket (100 burst / 10 per s).
    GetBlocks,
    /// Legacy `UPDATE_BLOCKCHAIN:` gossip pushes — main bucket only.
    ChainGossip,
    /// Anything else (junk `PING`, legacy `GET_BLOCKCHAIN`, unknown tags).
    Other,
}

/// Why a rate check failed.
///
/// * `Exceeded` — a bucket was empty: the message was refused (dropped).
///   Counts as one violation; the caller normally keeps the connection open
///   so the flood can keep accumulating violations.
/// * `Ban` — `ban_threshold` violations were reached inside `ban_window`, or
///   the peer carries an active fast-path ban: close the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitError {
    /// A bucket was exhausted — message refused (1 violation recorded).
    Exceeded,
    /// Violation threshold reached / peer currently banned.
    Ban,
}

impl fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RateLimitError::Exceeded => write!(f, "rate limit exceeded: bucket exhausted"),
            RateLimitError::Ban => write!(f, "peer banned: rate limit threshold reached"),
        }
    }
}

impl std::error::Error for RateLimitError {}

impl From<RateLimitError> for NetError {
    fn from(err: RateLimitError) -> Self {
        NetError::RateLimit(err.to_string())
    }
}

/// One bucket's parameters: `capacity` tokens of burst, `refill` tokens per
/// second of steady-state rate (ADR-0023 table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketSpec {
    /// Maximum tokens (burst size).
    pub capacity: u32,
    /// Tokens restored per second.
    pub refill: u32,
}

impl BucketSpec {
    /// A bucket with `capacity` burst and `refill` tokens/s.
    pub const fn new(capacity: u32, refill: u32) -> Self {
        BucketSpec { capacity, refill }
    }
}

/// Parse a `rate_limit_per_type` spec of the form `"100/10s"`:
/// capacity 100, window 10 s ⇒ refill = 100/10 = 10 tokens/s.
///
/// The window must divide the capacity evenly into at least 1 token/s
/// (`"5/10s"` would round to a bucket that never refills — rejected).
pub fn parse_rate_spec(spec: &str) -> Result<BucketSpec, String> {
    let spec = spec.trim();
    let core = spec
        .strip_suffix('s')
        .ok_or_else(|| format!("rate spec {spec:?} must end with 's' (e.g. \"100/10s\")"))?;
    let (cap_s, win_s) = core
        .split_once('/')
        .ok_or_else(|| format!("rate spec {spec:?} must look like \"100/10s\""))?;
    let capacity: u32 = cap_s
        .trim()
        .parse()
        .map_err(|_| format!("rate spec {spec:?}: capacity must be an integer"))?;
    let window: u32 = win_s
        .trim()
        .parse()
        .map_err(|_| format!("rate spec {spec:?}: window must be an integer"))?;
    if window == 0 {
        return Err(format!("rate spec {spec:?}: window must be > 0"));
    }
    if capacity == 0 {
        return Err(format!("rate spec {spec:?}: capacity must be > 0"));
    }
    let refill = capacity / window;
    if refill == 0 {
        return Err(format!(
            "rate spec {spec:?}: capacity must be >= window (refill >= 1/s)"
        ));
    }
    Ok(BucketSpec::new(capacity, refill))
}

/// Operator-facing tunables for the token-bucket model (`config.toml:
/// rate_limit_*`, ADR-0023 "Tuning surface"). Defaults reproduce the
/// ADR-0023 "Bucket parameters (initial)" table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Main bucket charged for every inbound wire message.
    pub main: BucketSpec,
    /// Legacy `GET_HEADERS` sync sub-bucket (Stage 1 carry-over).
    pub get_headers: BucketSpec,
    /// Legacy `GET_BLOCKS` sync sub-bucket (Stage 1 carry-over).
    pub get_blocks: BucketSpec,
    /// `INV` sub-bucket.
    pub inv: BucketSpec,
    /// `BLOCK` sub-bucket.
    pub block: BucketSpec,
    /// `TX` sub-bucket.
    pub tx: BucketSpec,
    /// Violations (inside `ban_window`) that trigger the fast-path ban.
    pub ban_threshold: u32,
    /// Rolling violation window.
    pub ban_window: Duration,
    /// Fast-path ban duration (ADR-0023: 5 minutes).
    pub ban_duration: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        RateLimitConfig {
            main: BucketSpec::new(1000, 100),
            get_headers: BucketSpec::new(100, 10),
            get_blocks: BucketSpec::new(100, 10),
            inv: BucketSpec::new(100, 10),
            block: BucketSpec::new(30, 3),
            tx: BucketSpec::new(1000, 100),
            ban_threshold: 5,
            ban_window: Duration::from_secs(60),
            ban_duration: Duration::from_secs(300),
        }
    }
}

/// Lazy-refill token bucket: `tokens` are restored arithmetically from the
/// time elapsed since `last_refill` — no background timers, no per-check
/// allocation (ADR-0023 "Algorithm").
///
/// Refill is credited in *whole* tokens; the sub-token remainder stays in
/// `last_refill` (it is only advanced by the time actually credited), so
/// frequent sub-second checks cannot starve the refill and slow checks cannot
/// over-credit past `capacity`.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Maximum tokens (burst).
    capacity: u32,
    /// Currently available tokens.
    tokens: u32,
    /// Tokens restored per second.
    refill_rate: u32,
    /// Last time refill tokens were credited.
    last_refill: Instant,
}

impl TokenBucket {
    /// A bucket that starts full (`capacity` tokens) at `now`.
    pub fn new(capacity: u32, refill_rate: u32, now: Instant) -> Self {
        TokenBucket {
            capacity,
            tokens: capacity,
            refill_rate,
            last_refill: now,
        }
    }

    /// Build from a [`BucketSpec`] (starts full at `now`).
    pub fn with_spec(spec: BucketSpec, now: Instant) -> Self {
        Self::new(spec.capacity, spec.refill, now)
    }

    /// Tokens currently available (without refilling).
    pub fn available(&self) -> u32 {
        self.tokens
    }

    /// Credit refill tokens for the time elapsed since `last_refill`.
    ///
    /// Pure arithmetic on the injected `now` (Stage 2 determinism checklist:
    /// wall clock enters only at the API edge).
    pub fn refill_at(&mut self, now: Instant) {
        if self.refill_rate == 0 {
            return;
        }
        if now <= self.last_refill {
            return;
        }
        let space = self.capacity - self.tokens;
        if space == 0 {
            // Bucket saturated: idle time is forfeited, not banked.
            self.last_refill = now;
            return;
        }
        let elapsed = (now - self.last_refill).as_secs_f64();
        let due = elapsed * f64::from(self.refill_rate);
        if due < 1.0 {
            return; // keep the sub-token remainder in last_refill
        }
        let space_f = f64::from(space);
        let add_f = due.min(space_f);
        let add = add_f as u32;
        self.tokens += add;
        self.last_refill += Duration::from_secs_f64(f64::from(add) / f64::from(self.refill_rate));
        if due > space_f {
            // Overflowed past capacity: the excess time is discarded.
            self.last_refill = now;
        }
    }

    /// Refill at the real clock, then consume `count` tokens if available.
    pub fn try_consume(&mut self, count: u32) -> bool {
        self.try_consume_at(Instant::now(), count)
    }

    /// Refill at the injected `now`, then consume `count` tokens if available.
    pub fn try_consume_at(&mut self, now: Instant, count: u32) -> bool {
        self.refill_at(now);
        if self.tokens >= count {
            self.tokens -= count;
            true
        } else {
            false
        }
    }

    /// Consume without refilling or checking — the caller must have verified
    /// [`available`](Self::available) >= `count` after a refill (used for the
    /// atomic main+type double charge).
    pub fn take(&mut self, count: u32) {
        debug_assert!(self.tokens >= count, "take() beyond available tokens");
        self.tokens = self.tokens.saturating_sub(count);
    }
}

/// Per-peer rate-limit state: main bucket + sub-buckets + violation bookkeeping.
///
/// Prompt S2-P16 sketch plus the ADR-0023 fields the sketch cannot express:
/// `banned_until`/`ban_duration` (the 5-minute fast-path ban) and the two
/// legacy sync buckets for [`MessageClass`].
#[derive(Debug)]
pub struct PeerRateLimiter {
    /// Charged for every inbound message.
    pub main_bucket: TokenBucket,
    /// Per-type sub-buckets (only types present in `rate_limit_per_type`).
    pub per_type_buckets: HashMap<MessageType, TokenBucket>,
    /// Legacy `GET_HEADERS` sub-bucket (ADR-0023 bucket table).
    pub sync_get_headers_bucket: TokenBucket,
    /// Legacy `GET_BLOCKS` sub-bucket (ADR-0023 bucket table).
    pub sync_get_blocks_bucket: TokenBucket,
    /// Violations inside `ban_window`.
    pub violation_count: u32,
    /// Time of the last violation (window decay anchor).
    pub last_violation: Option<Instant>,
    /// Violations that trigger the ban.
    pub ban_threshold: u32,
    /// Rolling violation window.
    pub ban_window: Duration,
    /// Fast-path ban duration.
    pub ban_duration: Duration,
    /// Active fast-path ban expiry (`None` = not banned).
    pub banned_until: Option<Instant>,
}

impl PeerRateLimiter {
    /// Fresh peer state from the config; buckets start full at `now`.
    pub fn new(config: &RateLimitConfig, now: Instant) -> Self {
        let mut per_type_buckets = HashMap::new();
        per_type_buckets.insert(MessageType::Inv, TokenBucket::with_spec(config.inv, now));
        per_type_buckets.insert(MessageType::Block, TokenBucket::with_spec(config.block, now));
        per_type_buckets.insert(MessageType::Tx, TokenBucket::with_spec(config.tx, now));
        PeerRateLimiter {
            main_bucket: TokenBucket::with_spec(config.main, now),
            per_type_buckets,
            sync_get_headers_bucket: TokenBucket::with_spec(config.get_headers, now),
            sync_get_blocks_bucket: TokenBucket::with_spec(config.get_blocks, now),
            violation_count: 0,
            last_violation: None,
            ban_threshold: config.ban_threshold,
            ban_window: config.ban_window,
            ban_duration: config.ban_duration,
            banned_until: None,
        }
    }

    /// Charge one message of `msg_type` (main bucket + sub-bucket if present)
    /// at the real clock.
    pub fn check_rate(&mut self, msg_type: MessageType) -> Result<(), RateLimitError> {
        self.check_rate_at(Instant::now(), msg_type)
    }

    /// [`check_rate`](Self::check_rate) with an injected clock.
    pub fn check_rate_at(
        &mut self,
        now: Instant,
        msg_type: MessageType,
    ) -> Result<(), RateLimitError> {
        self.guard(now)?;
        // Refill both buckets first so the availability check is coherent.
        self.main_bucket.refill_at(now);
        let type_tokens = match self.per_type_buckets.get_mut(&msg_type) {
            Some(bucket) => {
                bucket.refill_at(now);
                Some(bucket)
            }
            None => None,
        };
        let main_ok = self.main_bucket.available() >= 1;
        let type_ok = type_tokens.as_ref().is_none_or(|b| b.available() >= 1);
        if main_ok && type_ok {
            self.main_bucket.take(1);
            if let Some(b) = type_tokens {
                b.take(1);
            }
            Ok(())
        } else {
            self.record_violation(now)
        }
    }

    /// Main-bucket-only charge (accept gate / outbound sync rounds) at the
    /// real clock.
    pub fn check(&mut self) -> Result<(), RateLimitError> {
        self.check_at(Instant::now())
    }

    /// [`check`](Self::check) with an injected clock.
    pub fn check_at(&mut self, now: Instant) -> Result<(), RateLimitError> {
        self.guard(now)?;
        if self.main_bucket.try_consume_at(now, 1) {
            Ok(())
        } else {
            self.record_violation(now)
        }
    }

    /// Legacy [`MessageClass`] mapping (ADR-0023: sync classes charge their
    /// sub-buckets, `ChainGossip`/`Other` draw from the main bucket only).
    pub fn check_class(&mut self, class: MessageClass) -> Result<(), RateLimitError> {
        self.check_class_at(Instant::now(), class)
    }

    /// [`check_class`](Self::check_class) with an injected clock.
    pub fn check_class_at(
        &mut self,
        now: Instant,
        class: MessageClass,
    ) -> Result<(), RateLimitError> {
        self.guard(now)?;
        match class {
            MessageClass::GetHeaders => {
                self.main_bucket.refill_at(now);
                self.sync_get_headers_bucket.refill_at(now);
                if self.main_bucket.available() >= 1
                    && self.sync_get_headers_bucket.available() >= 1
                {
                    self.main_bucket.take(1);
                    self.sync_get_headers_bucket.take(1);
                    Ok(())
                } else {
                    self.record_violation(now)
                }
            }
            MessageClass::GetBlocks => {
                self.main_bucket.refill_at(now);
                self.sync_get_blocks_bucket.refill_at(now);
                if self.main_bucket.available() >= 1
                    && self.sync_get_blocks_bucket.available() >= 1
                {
                    self.main_bucket.take(1);
                    self.sync_get_blocks_bucket.take(1);
                    Ok(())
                } else {
                    self.record_violation(now)
                }
            }
            MessageClass::ChainGossip | MessageClass::Other => {
                // ADR-0023: legacy ChainGossip/Other map onto the main bucket.
                if self.main_bucket.try_consume_at(now, 1) {
                    Ok(())
                } else {
                    self.record_violation(now)
                }
            }
        }
    }

    /// Set the fast-path ban (`ban_duration`, ADR-0023: 5 min) — explicit
    /// bans from network-id mismatch / inbox overflow.
    pub fn ban(&mut self, now: Instant) {
        self.banned_until = Some(now + self.ban_duration);
    }

    /// Whether an active fast-path ban is in effect at `now`.
    pub fn is_banned_at(&self, now: Instant) -> bool {
        self.banned_until.is_some_and(|until| now < until)
    }

    /// Real-clock variant of [`is_banned_at`](Self::is_banned_at).
    pub fn is_banned(&self) -> bool {
        self.is_banned_at(Instant::now())
    }

    /// Violations recorded inside the window (observability + tests).
    pub fn violations(&self) -> u32 {
        self.violation_count
    }

    /// Ban-state pre-check: reject while banned, clear an expired ban together
    /// with its violation counters (Stage 1 `check_and_clear_ban` semantics).
    fn guard(&mut self, now: Instant) -> Result<(), RateLimitError> {
        if let Some(until) = self.banned_until {
            if now < until {
                return Err(RateLimitError::Ban);
            }
            self.banned_until = None;
            self.violation_count = 0;
            self.last_violation = None;
        }
        Ok(())
    }

    /// One refused message = one ADR-0023 violation; the counter decays after
    /// `ban_window` of quiet; reaching `ban_threshold` arms the ban.
    fn record_violation(&mut self, now: Instant) -> Result<(), RateLimitError> {
        if let Some(last) = self.last_violation {
            if now.saturating_duration_since(last) >= self.ban_window {
                self.violation_count = 0;
            }
        }
        self.violation_count += 1;
        self.last_violation = Some(now);
        if self.violation_count >= self.ban_threshold {
            self.banned_until = Some(now + self.ban_duration);
            Err(RateLimitError::Ban)
        } else {
            Err(RateLimitError::Exceeded)
        }
    }
}

/// Thread-safe container keyed by `SocketAddr` (ADR-0023: works pre-HELLO).
///
/// The monolith holds `Arc<RateLimiter>`; every check method resolves the
/// peer entry lazily (entry creation = buckets start full at `now`).
pub struct RateLimiter {
    config: RateLimitConfig,
    peers: Mutex<HashMap<SocketAddr, PeerRateLimiter>>,
}

impl RateLimiter {
    /// Build a limiter with explicit tunables (`config.toml` mapping lives in
    /// the root crate's `Config::rate_limit_config`).
    pub fn new(config: RateLimitConfig) -> Self {
        RateLimiter {
            config,
            peers: Mutex::new(HashMap::new()),
        }
    }

    /// The ADR-0023 default tunables.
    pub fn with_defaults() -> Self {
        Self::new(RateLimitConfig::default())
    }

    /// Current tunables (read-only).
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    fn peer_entry<'a>(
        &self,
        peers: &'a mut HashMap<SocketAddr, PeerRateLimiter>,
        addr: SocketAddr,
        now: Instant,
    ) -> &'a mut PeerRateLimiter {
        peers
            .entry(addr)
            .or_insert_with(|| PeerRateLimiter::new(&self.config, now))
    }

    /// Typed charge: main bucket + `msg_type` sub-bucket (real clock).
    pub fn check_rate(&self, addr: SocketAddr, msg_type: MessageType) -> Result<(), RateLimitError> {
        self.check_rate_at(Instant::now(), addr, msg_type)
    }

    /// [`check_rate`](Self::check_rate) with an injected clock.
    pub fn check_rate_at(
        &self,
        now: Instant,
        addr: SocketAddr,
        msg_type: MessageType,
    ) -> Result<(), RateLimitError> {
        let mut peers = self.peers.lock().unwrap();
        self.peer_entry(&mut peers, addr, now)
            .check_rate_at(now, msg_type)
    }

    /// Main-bucket-only charge (real clock).
    pub fn check(&self, addr: SocketAddr) -> Result<(), RateLimitError> {
        self.check_at(Instant::now(), addr)
    }

    /// [`check`](Self::check) with an injected clock.
    pub fn check_at(&self, now: Instant, addr: SocketAddr) -> Result<(), RateLimitError> {
        let mut peers = self.peers.lock().unwrap();
        self.peer_entry(&mut peers, addr, now).check_at(now)
    }

    /// Legacy [`MessageClass`] charge (real clock).
    pub fn check_class(
        &self,
        addr: SocketAddr,
        class: MessageClass,
    ) -> Result<(), RateLimitError> {
        self.check_class_at(Instant::now(), addr, class)
    }

    /// [`check_class`](Self::check_class) with an injected clock.
    pub fn check_class_at(
        &self,
        now: Instant,
        addr: SocketAddr,
        class: MessageClass,
    ) -> Result<(), RateLimitError> {
        let mut peers = self.peers.lock().unwrap();
        self.peer_entry(&mut peers, addr, now)
            .check_class_at(now, class)
    }

    /// Explicit fast-path ban (network-id mismatch, inbox overflow).
    pub fn ban(&self, addr: SocketAddr) {
        self.ban_at(Instant::now(), addr);
    }

    /// [`ban`](Self::ban) with an injected clock.
    pub fn ban_at(&self, now: Instant, addr: SocketAddr) {
        let mut peers = self.peers.lock().unwrap();
        self.peer_entry(&mut peers, addr, now).ban(now);
    }

    /// Whether the address carries an active fast-path ban (real clock).
    pub fn is_banned(&self, addr: SocketAddr) -> bool {
        self.is_banned_at(Instant::now(), addr)
    }

    /// [`is_banned`](Self::is_banned) with an injected clock. Unknown
    /// addresses are not banned (no entry created).
    pub fn is_banned_at(&self, now: Instant, addr: SocketAddr) -> bool {
        let peers = self.peers.lock().unwrap();
        peers
            .get(&addr)
            .is_some_and(|peer| peer.is_banned_at(now))
    }

    /// Violations recorded for `addr` (0 for unknown addresses) — the S2-P16
    /// test observability hook.
    pub fn violation_count(&self, addr: SocketAddr) -> u32 {
        let peers = self.peers.lock().unwrap();
        peers.get(&addr).map_or(0, |peer| peer.violations())
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimiter")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn token_bucket_starts_full_and_refills_lazily() {
        let now = t0();
        let mut bucket = TokenBucket::new(10, 2, now);
        assert_eq!(bucket.available(), 10);
        assert!(bucket.try_consume_at(now, 10));
        assert_eq!(bucket.available(), 0);
        // Sub-second: no whole token due yet.
        assert!(!bucket.try_consume_at(now + Duration::from_millis(400), 1));
        // +0.6s -> 2 * 0.6 = 1.2 -> 1 whole token.
        let t = now + Duration::from_millis(600);
        assert!(bucket.try_consume_at(t, 1));
        assert_eq!(bucket.available(), 0);
        // Remainder kept: +0.4s more -> due = 2*1.0 - credited 1 = 1 token.
        let t2 = now + Duration::from_millis(1000);
        assert!(bucket.try_consume_at(t2, 1));
        assert_eq!(bucket.available(), 0);
    }

    #[test]
    fn token_bucket_never_refills_past_capacity() {
        let now = t0();
        let mut bucket = TokenBucket::new(10, 100, now);
        // An hour of idle time must not bank more than `capacity`.
        bucket.refill_at(now + Duration::from_secs(3600));
        assert_eq!(bucket.available(), 10);
        assert!(bucket.try_consume_at(now + Duration::from_secs(3600), 10));
        assert_eq!(bucket.available(), 0);
        // +1s at 100/s would grant 100 tokens — capacity caps it at 10.
        bucket.refill_at(now + Duration::from_secs(3601));
        assert_eq!(bucket.available(), 10, "refill capped at capacity");
        // And no more than that on a second idle stretch.
        bucket.refill_at(now + Duration::from_secs(4000));
        assert_eq!(bucket.available(), 10);
    }

    #[test]
    fn token_bucket_refill_accounting_is_exact_over_repeated_checks() {
        let now = t0();
        let mut bucket = TokenBucket::new(100, 10, now);
        assert!(bucket.try_consume_at(now, 100));
        assert_eq!(bucket.available(), 0);
        // 10 checks x 100ms = 1.0s elapsed -> exactly 10 tokens.
        for i in 1..=10 {
            let t = now + Duration::from_millis(100 * i);
            bucket.refill_at(t);
        }
        assert_eq!(bucket.available(), 10);
    }

    #[test]
    fn violation_window_decays_and_threshold_arms_ban() {
        let now = t0();
        let mut peer = PeerRateLimiter::new(&RateLimitConfig::default(), now);
        // Exhaust the INV sub-bucket (capacity 100 default).
        for _ in 0..100 {
            peer.check_rate_at(now, MessageType::Inv).unwrap();
        }
        // Violations 1..4 are Exceeded, the 5th (threshold) is Ban.
        for i in 1..5 {
            assert_eq!(
                peer.check_rate_at(now, MessageType::Inv),
                Err(RateLimitError::Exceeded),
                "violation {i} stays below the threshold"
            );
        }
        assert_eq!(
            peer.check_rate_at(now, MessageType::Inv),
            Err(RateLimitError::Ban),
            "violation 5 reaches ban_threshold"
        );
        assert_eq!(peer.violations(), 5);
        assert!(peer.is_banned_at(now));
        // While banned every check is Ban and violations do not grow.
        assert_eq!(
            peer.check_rate_at(now, MessageType::Tx),
            Err(RateLimitError::Ban)
        );
        assert_eq!(peer.violations(), 5);
        // After ban_duration the ban clears together with the counters —
        // lazily, on the next check (is_banned_at only peeks).
        let after = now + Duration::from_secs(301);
        assert!(!peer.is_banned_at(after));
        // INV bucket has refilled during those 5 minutes -> accepts again.
        peer.check_rate_at(after, MessageType::Inv).unwrap();
        assert_eq!(peer.violations(), 0, "counters cleared with the ban");
    }

    #[test]
    fn violation_counter_decays_after_window_of_quiet() {
        let now = t0();
        let cfg = RateLimitConfig {
            ban_threshold: 5,
            ban_window: Duration::from_secs(60),
            // A non-refilling bucket keeps failing after the quiet period so
            // the *violation counter* (not a recovered bucket) is under test.
            inv: BucketSpec::new(100, 0),
            ..RateLimitConfig::default()
        };
        let mut peer = PeerRateLimiter::new(&cfg, now);
        for _ in 0..100 {
            peer.check_rate_at(now, MessageType::Inv).unwrap();
        }
        // 3 violations now...
        for _ in 0..3 {
            assert_eq!(
                peer.check_rate_at(now, MessageType::Inv),
                Err(RateLimitError::Exceeded)
            );
        }
        assert_eq!(peer.violations(), 3);
        // ...61s of quiet later the counter has decayed: a fresh violation
        // starts over at 1 instead of reaching the threshold.
        let later = now + Duration::from_secs(61);
        assert_eq!(
            peer.check_rate_at(later, MessageType::Inv),
            Err(RateLimitError::Exceeded)
        );
        assert_eq!(peer.violations(), 1);
        assert!(!peer.is_banned_at(later));
    }

    #[test]
    fn per_type_isolation_exhausting_block_keeps_tx_open() {
        let now = t0();
        let cfg = RateLimitConfig {
            block: BucketSpec::new(3, 1),
            tx: BucketSpec::new(3, 1),
            ..RateLimitConfig::default()
        };
        let mut peer = PeerRateLimiter::new(&cfg, now);
        for _ in 0..3 {
            peer.check_rate_at(now, MessageType::Block).unwrap();
        }
        assert_eq!(
            peer.check_rate_at(now, MessageType::Block),
            Err(RateLimitError::Exceeded)
        );
        // TX sub-bucket untouched: still accepts (main has 1000-4 tokens).
        peer.check_rate_at(now, MessageType::Tx).unwrap();
        peer.check_rate_at(now, MessageType::Tx).unwrap();
        // A type without a configured sub-bucket draws from main only.
        peer.check_rate_at(now, MessageType::Hello).unwrap();
        assert_eq!(peer.violations(), 1);
    }

    #[test]
    fn legacy_class_mapping_sync_buckets_and_main_only() {
        let now = t0();
        let cfg = RateLimitConfig {
            get_headers: BucketSpec::new(3, 1),
            get_blocks: BucketSpec::new(3, 1),
            ..RateLimitConfig::default()
        };
        let mut peer = PeerRateLimiter::new(&cfg, now);
        for _ in 0..3 {
            peer.check_class_at(now, MessageClass::GetHeaders).unwrap();
        }
        assert_eq!(
            peer.check_class_at(now, MessageClass::GetHeaders),
            Err(RateLimitError::Exceeded),
            "GET_HEADERS sub-bucket exhausted"
        );
        // GetBlocks has its own bucket: unaffected.
        peer.check_class_at(now, MessageClass::GetBlocks).unwrap();
        // ChainGossip/Other charge main only (ADR-0023 mapping).
        peer.check_class_at(now, MessageClass::ChainGossip).unwrap();
        peer.check_class_at(now, MessageClass::Other).unwrap();
        assert_eq!(peer.violations(), 1);
    }

    #[test]
    fn explicit_ban_blocks_all_checks_until_duration() {
        let now = t0();
        let mut peer = PeerRateLimiter::new(&RateLimitConfig::default(), now);
        peer.ban(now);
        assert!(peer.is_banned_at(now));
        assert_eq!(peer.check_at(now), Err(RateLimitError::Ban));
        assert_eq!(
            peer.check_rate_at(now, MessageType::Tx),
            Err(RateLimitError::Ban)
        );
        assert_eq!(
            peer.check_class_at(now, MessageClass::Other),
            Err(RateLimitError::Ban)
        );
        // 299s later still banned; 301s later the ban has expired.
        assert!(peer.is_banned_at(now + Duration::from_secs(299)));
        assert!(!peer.is_banned_at(now + Duration::from_secs(301)));
        peer.check_at(now + Duration::from_secs(301)).unwrap();
    }

    #[test]
    fn container_tracks_peers_independently_and_lazily() {
        let limiter = RateLimiter::default();
        let now = t0();
        let a: SocketAddr = "10.0.0.1:8081".parse().unwrap();
        let b: SocketAddr = "10.0.0.2:8081".parse().unwrap();
        // Unknown addresses report 0 violations and no ban.
        assert_eq!(limiter.violation_count(a), 0);
        assert!(!limiter.is_banned_at(now, a));

        // Ban addr A explicitly; B keeps working.
        limiter.ban_at(now, a);
        assert!(limiter.is_banned_at(now, a));
        assert_eq!(limiter.check_at(now, a), Err(RateLimitError::Ban));
        limiter.check_at(now, b).unwrap();
        // Violation counters are per-address.
        assert_eq!(limiter.violation_count(a), 0);
    }

    #[test]
    fn container_typed_check_charges_main_and_sub_bucket() {
        let limiter = RateLimiter::default();
        let now = t0();
        let addr: SocketAddr = "10.0.0.3:8081".parse().unwrap();
        // Default INV bucket: 100 burst.
        for _ in 0..100 {
            limiter.check_rate_at(now, addr, MessageType::Inv).unwrap();
        }
        assert_eq!(
            limiter.check_rate_at(now, addr, MessageType::Inv),
            Err(RateLimitError::Exceeded)
        );
        assert_eq!(limiter.violation_count(addr), 1);
        // Main bucket was only charged 100 of 1000: a main-only type works.
        limiter.check_rate_at(now, addr, MessageType::Hello).unwrap();
    }

    #[test]
    fn prompt_burst_semantics_via_container() {
        // КГ: 200 messages inside main capacity -> OK; 1500 -> exceeded.
        // INV has a 100-token sub-bucket by default, so the burst test runs
        // against a type without a sub-bucket (documented deviation: literal
        // "200 INV OK" contradicts INV=100 burst of ADR-0023).
        let limiter = RateLimiter::default();
        let now = t0();
        let addr: SocketAddr = "10.0.0.4:8081".parse().unwrap();
        for _ in 0..200 {
            limiter.check_rate_at(now, addr, MessageType::Hello).unwrap();
        }
        assert_eq!(limiter.violation_count(addr), 0);

        let addr2: SocketAddr = "10.0.0.5:8081".parse().unwrap();
        let mut banned = false;
        for _ in 0..1500 {
            match limiter.check_rate_at(now, addr2, MessageType::Hello) {
                Ok(()) => {}
                Err(RateLimitError::Exceeded) => {}
                Err(RateLimitError::Ban) => {
                    banned = true;
                    break;
                }
            }
        }
        assert!(banned, "1500 > capacity 1000 must ban after 5 violations");
        assert!(limiter.is_banned_at(now, addr2));
        assert_eq!(limiter.violation_count(addr2), 5);
    }

    #[test]
    fn parse_rate_spec_accepts_and_rejects() {
        assert_eq!(parse_rate_spec("100/10s"), Ok(BucketSpec::new(100, 10)));
        assert_eq!(parse_rate_spec("30/10s"), Ok(BucketSpec::new(30, 3)));
        assert_eq!(parse_rate_spec("1000/10s"), Ok(BucketSpec::new(1000, 100)));
        assert!(parse_rate_spec("abc").is_err());
        assert!(parse_rate_spec("100/10").is_err(), "missing s suffix");
        assert!(parse_rate_spec("100/0s").is_err(), "zero window");
        assert!(parse_rate_spec("0/10s").is_err(), "zero capacity");
        assert!(parse_rate_spec("5/10s").is_err(), "refill would round to 0");
        assert!(parse_rate_spec("100/").is_err());
    }

    #[test]
    fn default_config_matches_adr0023_table() {
        let cfg = RateLimitConfig::default();
        assert_eq!(cfg.main, BucketSpec::new(1000, 100));
        assert_eq!(cfg.inv, BucketSpec::new(100, 10));
        assert_eq!(cfg.block, BucketSpec::new(30, 3));
        assert_eq!(cfg.tx, BucketSpec::new(1000, 100));
        assert_eq!(cfg.get_headers, BucketSpec::new(100, 10));
        assert_eq!(cfg.get_blocks, BucketSpec::new(100, 10));
        assert_eq!(cfg.ban_threshold, 5);
        assert_eq!(cfg.ban_window, Duration::from_secs(60));
        assert_eq!(cfg.ban_duration, Duration::from_secs(300));
    }
}
