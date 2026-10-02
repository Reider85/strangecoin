use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender, TrySendError};
use std::sync::Mutex;
use std::time::Duration;
use tracing::debug;

/// Poll cadence of the crossbeam -> tokio bridge in [`EventBus::subscribe_async`].
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug)]
pub enum NodeEvent {
    BlockApplied { height: u64, hash: String },
    BlockReorged { old_tip: String, new_tip: String },
    TxAccepted { txid: String },
    TxRejected { txid: String, reason: String },
    MiningStarted,
    MiningFinished,
    PeerScoreChanged { peer: String, score: i64 },
    StatePersisted { height: u64 },
}

pub struct EventBus {
    subscribers: Mutex<Vec<Sender<NodeEvent>>>,
}

impl EventBus {
    pub fn new() -> Self {
        EventBus {
            subscribers: Mutex::new(Vec::new()),
        }
    }

    pub fn subscribe(&self) -> Receiver<NodeEvent> {
        let (tx, rx) = unbounded();
        let mut subs = self.subscribers.lock().expect("EventBus lock poisoned");
        subs.push(tx);
        rx
    }

    /// Async bridge for tokio consumers (ADR-0007).
    ///
    /// Producers stay synchronous (mining, P2P threads) and keep publishing into
    /// crossbeam; this pumps those events into a `tokio::sync::mpsc` channel so
    /// `async` subsystems can `recv().await` without blocking a runtime worker.
    ///
    /// The pump is a blocking task, not an async task: `crossbeam`'s
    /// `recv_timeout` cannot be cancelled by the runtime, so running it inside
    /// `tokio::spawn` would block the worker thread for the whole poll interval
    /// and starve every other task on that worker (including the consumer that
    /// is waiting for the event). `spawn_blocking` keeps the blocking wait off
    /// the runtime workers entirely.
    ///
    /// Subscribers inherit up to one interval of latency. That is fine for the
    /// intended consumer (S1-P18 SyncEngine) but latency-sensitive subscribers
    /// should keep using [`EventBus::subscribe`].
    pub fn subscribe_async(&self, capacity: usize) -> tokio::sync::mpsc::Receiver<NodeEvent> {
        let crossbeam_rx = self.subscribe();
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        tokio::task::spawn_blocking(move || loop {
            match crossbeam_rx.recv_timeout(POLL_INTERVAL) {
                Ok(event) => {
                    if tx.blocking_send(event).is_err() {
                        debug!("Async EventBus subscriber dropped, stopping bridge");
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    debug!("EventBus publisher dropped, stopping bridge");
                    break;
                }
            }
        });
        rx
    }

    pub fn publish(&self, event: NodeEvent) {
        let mut subs = self.subscribers.lock().expect("EventBus lock poisoned");
        subs.retain(|tx| {
            if let Err(TrySendError::Disconnected(_)) = tx.try_send(event.clone()) {
                debug!("Removing disconnected EventBus subscriber");
                false
            } else {
                true
            }
        });
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers
            .lock()
            .expect("EventBus lock poisoned")
            .len()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn three_subscribers_receive_all_events() {
        let bus = EventBus::new();
        let rx1 = bus.subscribe();
        let rx2 = bus.subscribe();
        let rx3 = bus.subscribe();

        bus.publish(NodeEvent::MiningStarted);
        bus.publish(NodeEvent::BlockApplied {
            height: 1,
            hash: "abc".into(),
        });

        for rx in [&rx1, &rx2, &rx3] {
            let e1 = rx.recv_timeout(Duration::from_millis(100)).unwrap();
            let e2 = rx.recv_timeout(Duration::from_millis(100)).unwrap();
            assert!(matches!(e1, NodeEvent::MiningStarted));
            assert!(matches!(e2, NodeEvent::BlockApplied { height: 1, .. }));
        }
    }

    #[test]
    fn slow_subscriber_does_not_block_publish() {
        let bus = EventBus::new();
        let _rx = bus.subscribe();

        let slow_bus = EventBus::new();
        let _slow_rx = slow_bus.subscribe();
        drop(_slow_rx);

        for i in 0..100 {
            slow_bus.publish(NodeEvent::BlockApplied {
                height: i,
                hash: "x".into(),
            });
        }
    }

    #[test]
    fn reorg_generates_block_reorged() {
        let bus = EventBus::new();
        let rx = bus.subscribe();

        bus.publish(NodeEvent::BlockReorged {
            old_tip: "aaa".into(),
            new_tip: "bbb".into(),
        });

        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            NodeEvent::BlockReorged { old_tip, new_tip } => {
                assert_eq!(old_tip, "aaa");
                assert_eq!(new_tip, "bbb");
            }
            _ => panic!("Expected BlockReorged"),
        }
    }

    #[test]
    fn disconnected_subscriber_is_removed() {
        let bus = EventBus::new();
        let _rx = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);

        drop(_rx);
        bus.publish(NodeEvent::MiningStarted);
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn async_bridge_delivers_events() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_async(8);

        bus.publish(NodeEvent::BlockApplied {
            height: 7,
            hash: "deadbeef".into(),
        });
        bus.publish(NodeEvent::MiningFinished);

        let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("bridge timed out")
            .expect("bridge closed early");
        assert!(matches!(first, NodeEvent::BlockApplied { height: 7, .. }));

        let second = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("bridge timed out")
            .expect("bridge closed early");
        assert!(matches!(second, NodeEvent::MiningFinished));
    }

    #[tokio::test]
    async fn async_bridge_does_not_block_publish() {
        let bus = EventBus::new();
        // Subscriber that never reads: the bounded async channel fills up, but
        // publish() must still return promptly.
        let _rx = bus.subscribe_async(1);

        for i in 0..100u64 {
            bus.publish(NodeEvent::BlockApplied {
                height: i,
                hash: "x".into(),
            });
        }
    }
}
