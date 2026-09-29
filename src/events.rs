use crossbeam_channel::{unbounded, Receiver, SendError, Sender};
use std::sync::Mutex;
use tracing::debug;

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

    pub fn publish(&self, event: NodeEvent) {
        let mut subs = self.subscribers.lock().expect("EventBus lock poisoned");
        subs.retain(|tx| {
            if let Err(SendError(_)) = tx.try_send(event.clone()) {
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
}
