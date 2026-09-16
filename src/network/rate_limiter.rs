use crate::error::StrangecoinError;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct PeerCounter {
    count: usize,
    window_start: Instant,
    banned_until: Option<Instant>,
}

impl PeerCounter {
    fn new() -> Self {
        PeerCounter {
            count: 0,
            window_start: Instant::now(),
            banned_until: None,
        }
    }

    fn reset_window(&mut self, now: Instant) {
        self.count = 0;
        self.window_start = now;
    }

    fn is_banned(&self, now: Instant) -> bool {
        self.banned_until.map_or(false, |until| now < until)
    }

    fn check_and_clear_ban(&mut self, now: Instant) -> bool {
        if let Some(until) = self.banned_until {
            if now >= until {
                self.banned_until = None;
                self.count = 0;
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

    pub fn check(&self, addr: SocketAddr) -> Result<(), StrangecoinError> {
        let mut peers = self.peers.lock().unwrap();
        let now = Instant::now();

        let counter = peers.entry(addr).or_insert_with(PeerCounter::new);

        if counter.check_and_clear_ban(now) {
            // Ban expired, counter was reset
        } else if counter.is_banned(now) {
            return Err(StrangecoinError::PeerBanned);
        }

        if now.duration_since(counter.window_start) >= self.window {
            counter.reset_window(now);
        }

        counter.count += 1;

        if counter.count > self.max_messages_per_window {
            counter.ban(self.ban_duration);
            tracing::warn!(peer = %addr, "Peer banned for exceeding rate limit");
            return Err(StrangecoinError::PeerBanned);
        }

        Ok(())
    }

    #[cfg(test)]
    pub fn is_banned(&self, addr: SocketAddr) -> bool {
        let peers = self.peers.lock().unwrap();
        let now = Instant::now();
        peers.get(&addr).map_or(false, |c| c.is_banned(now))
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
}
