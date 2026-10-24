use crate::error::StrangecoinError;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Wire-message class for per-class rate limiting (BUG-S1-005, P13):
/// different traffic types get independent buckets so a flooder of one class
/// cannot starve legit traffic of another before the ban threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageClass {
    /// `MSG_GET_HEADERS` — headers-first sync pulls.
    GetHeaders = 0,
    /// `MSG_GET_BLOCKS` — headers-first sync pulls.
    GetBlocks = 1,
    /// `UPDATE_BLOCKCHAIN:` gossip pushes (one per sync round per peer).
    ChainGossip = 2,
    /// Anything else: legacy `GET_BLOCKCHAIN` and unknown/junk text.
    Other = 3,
}

const NUM_CLASSES: usize = 4;

/// Per-class message caps inside the same sliding window as the total
/// bucket. The total bucket (`max_messages_per_window`) still applies on top;
/// a class cap trips the ban earlier when a single class is flooded.
const CLASS_LIMITS: [usize; NUM_CLASSES] = [
    100, // GetHeaders
    100, // GetBlocks
    30,  // ChainGossip (legit gossip ≈ 10 per 10s window per peer)
    30,  // Other (junk / legacy GET_BLOCKCHAIN)
];

struct PeerCounter {
    count: usize,
    class_counts: [usize; NUM_CLASSES],
    window_start: Instant,
    banned_until: Option<Instant>,
}

impl PeerCounter {
    fn new() -> Self {
        PeerCounter {
            count: 0,
            class_counts: [0; NUM_CLASSES],
            window_start: Instant::now(),
            banned_until: None,
        }
    }

    fn reset_window(&mut self, now: Instant) {
        self.count = 0;
        self.class_counts = [0; NUM_CLASSES];
        self.window_start = now;
    }

    fn is_banned(&self, now: Instant) -> bool {
        self.banned_until.is_some_and(|until| now < until)
    }

    fn check_and_clear_ban(&mut self, now: Instant) -> bool {
        if let Some(until) = self.banned_until {
            if now >= until {
                self.banned_until = None;
                self.count = 0;
                self.class_counts = [0; NUM_CLASSES];
                self.window_start = now;
                return true;
            }
        }
        false
    }

    fn ban(&mut self, duration: Duration) {
        self.banned_until = Some(Instant::now() + duration);
    }
}

pub struct RateLimiter {
    window: Duration,
    max_messages_per_window: usize,
    ban_duration: Duration,
    peers: Mutex<HashMap<SocketAddr, PeerCounter>>,
}

impl RateLimiter {
    pub fn new(window_secs: u64, max_messages_per_window: usize) -> Self {
        RateLimiter {
            window: Duration::from_secs(window_secs),
            max_messages_per_window,
            ban_duration: Duration::from_secs(300), // 5 minutes default
            peers: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    pub fn with_ban_duration(
        window_secs: u64,
        max_messages_per_window: usize,
        ban_duration: Duration,
    ) -> Self {
        RateLimiter {
            window: Duration::from_secs(window_secs),
            max_messages_per_window,
            ban_duration,
            peers: Mutex::new(HashMap::new()),
        }
    }

    /// Total per-peer bucket check (pre-HELLO gate, outbound sync rounds).
    pub fn check(&self, addr: SocketAddr) -> Result<(), StrangecoinError> {
        self.with_checked_counter(addr, |counter| {
            counter.count += 1;
            if counter.count > self.max_messages_per_window {
                counter.ban(self.ban_duration);
                tracing::warn!(peer = %addr, "Peer banned for exceeding rate limit");
                return Err(StrangecoinError::PeerBanned);
            }
            Ok(())
        })
    }

    /// Per-message check against BOTH the total bucket and the class bucket
    /// (BUG-S1-005: P13 per-message rate limiting). Exceeding either trips
    /// the same ban as [`check`](Self::check).
    pub fn check_class(
        &self,
        addr: SocketAddr,
        class: MessageClass,
    ) -> Result<(), StrangecoinError> {
        self.with_checked_counter(addr, |counter| {
            counter.count += 1;
            let class_idx = class as usize;
            counter.class_counts[class_idx] += 1;
            if counter.count > self.max_messages_per_window {
                counter.ban(self.ban_duration);
                tracing::warn!(peer = %addr, ?class, "Peer banned for exceeding rate limit");
                return Err(StrangecoinError::PeerBanned);
            }
            if counter.class_counts[class_idx] > CLASS_LIMITS[class_idx] {
                counter.ban(self.ban_duration);
                tracing::warn!(
                    peer = %addr,
                    ?class,
                    "Peer banned for exceeding per-class rate limit"
                );
                return Err(StrangecoinError::PeerBanned);
            }
            Ok(())
        })
    }

    /// Shared ban/window bookkeeping for [`check`](Self::check) and
    /// [`check_class`](Self::check_class): expired bans are cleared, active
    /// bans reject, stale windows roll over — then `f` applies the
    /// counter-specific increment and limit.
    fn with_checked_counter<R>(
        &self,
        addr: SocketAddr,
        f: impl FnOnce(&mut PeerCounter) -> Result<R, StrangecoinError>,
    ) -> Result<R, StrangecoinError> {
        let mut peers = self.peers.lock().unwrap();
        let now = Instant::now();

        let counter = peers.entry(addr).or_insert_with(PeerCounter::new);

        counter.check_and_clear_ban(now);
        if counter.is_banned(now) {
            return Err(StrangecoinError::PeerBanned);
        }

        if now.duration_since(counter.window_start) >= self.window {
            counter.reset_window(now);
        }

        f(counter)
    }

    /// Ban a specific peer by address, useful for protocol violations
    pub fn ban(&self, addr: SocketAddr) {
        let mut peers = self.peers.lock().unwrap();
        if let Some(counter) = peers.get_mut(&addr) {
            counter.ban(self.ban_duration);
            tracing::warn!(peer = %addr, "Peer banned for protocol violation");
        }
    }

    #[cfg(test)]
    pub fn is_banned(&self, addr: SocketAddr) -> bool {
        let peers = self.peers.lock().unwrap();
        let now = Instant::now();
        peers.get(&addr).is_some_and(|c| c.is_banned(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::thread::sleep;

    fn test_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
    }

    #[test]
    fn test_rate_limit_exceeded() {
        let limiter = RateLimiter::new(10, 100);
        let addr = test_addr(18080);

        for i in 1..=100 {
            assert!(limiter.check(addr).is_ok(), "Request {} should succeed", i);
        }

        assert!(limiter.check(addr).is_err(), "Request 101 should fail");
        assert!(matches!(
            limiter.check(addr),
            Err(StrangecoinError::PeerBanned)
        ));
    }

    #[test]
    fn test_banned_peer_rejected() {
        let limiter = RateLimiter::new(10, 5);
        let addr = test_addr(18081);

        for _ in 0..5 {
            limiter.check(addr).unwrap();
        }
        limiter.check(addr).unwrap_err();

        assert!(matches!(
            limiter.check(addr),
            Err(StrangecoinError::PeerBanned)
        ));
    }

    #[test]
    fn test_ban_expires() {
        let limiter = RateLimiter::with_ban_duration(1, 2, Duration::from_millis(500));
        let addr = test_addr(18082);

        limiter.check(addr).unwrap();
        limiter.check(addr).unwrap();
        limiter.check(addr).unwrap_err();

        assert!(limiter.is_banned(addr));

        sleep(Duration::from_millis(600));

        assert!(!limiter.is_banned(addr));
        assert!(limiter.check(addr).is_ok());
    }

    #[test]
    fn test_independent_peers() {
        let limiter = RateLimiter::new(10, 2);
        let addr1 = test_addr(18083);
        let addr2 = test_addr(18084);

        limiter.check(addr1).unwrap();
        limiter.check(addr1).unwrap();
        assert!(limiter.check(addr1).is_err());

        assert!(limiter.check(addr2).is_ok());
        assert!(limiter.check(addr2).is_ok());
        assert!(limiter.check(addr2).is_err());
    }

    #[test]
    fn test_per_class_limit_bans() {
        let limiter = RateLimiter::new(10, 100);
        let addr = test_addr(18085);

        // Other class cap is 30 (CLASS_LIMITS) — below the total cap of 100.
        for i in 1..=30 {
            assert!(
                limiter.check_class(addr, MessageClass::Other).is_ok(),
                "Other message {} should succeed",
                i
            );
        }
        assert!(
            matches!(
                limiter.check_class(addr, MessageClass::Other),
                Err(StrangecoinError::PeerBanned)
            ),
            "Other message 31 must trip the per-class ban"
        );
        // The ban is shared: the total-bucket entry point sees it too.
        assert!(matches!(
            limiter.check(addr),
            Err(StrangecoinError::PeerBanned)
        ));
    }

    #[test]
    fn test_class_isolation() {
        let limiter = RateLimiter::new(10, 100);
        let addr = test_addr(18086);

        // Exhaust the Other class budget (30)…
        for _ in 0..30 {
            limiter.check_class(addr, MessageClass::Other).unwrap();
        }
        // …a different class still has its own budget: with a shared class
        // counter this GetHeaders message would be message #31 and fail.
        assert!(limiter.check_class(addr, MessageClass::GetHeaders).is_ok());
        assert!(limiter.check_class(addr, MessageClass::GetHeaders).is_ok());
    }

    #[test]
    fn test_window_reset_clears_class_counters() {
        let limiter = RateLimiter::new(1, 100);
        let addr = test_addr(18087);

        for _ in 0..30 {
            limiter
                .check_class(addr, MessageClass::ChainGossip)
                .unwrap();
        }

        sleep(Duration::from_millis(1100));

        // A fresh window starts with cleared per-class counters.
        for _ in 0..30 {
            limiter
                .check_class(addr, MessageClass::ChainGossip)
                .unwrap();
        }
    }

    #[test]
    fn test_check_class_rejects_while_banned() {
        let limiter = RateLimiter::new(10, 2);
        let addr = test_addr(18088);

        limiter.check_class(addr, MessageClass::GetBlocks).unwrap();
        limiter.check_class(addr, MessageClass::GetBlocks).unwrap();
        assert!(matches!(
            limiter.check_class(addr, MessageClass::GetBlocks),
            Err(StrangecoinError::PeerBanned)
        ));
        assert!(matches!(
            limiter.check_class(addr, MessageClass::GetHeaders),
            Err(StrangecoinError::PeerBanned)
        ));
    }
}
