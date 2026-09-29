pub use strangecoin::test_support::*;
use strangecoin::events::{EventBus, NodeEvent};
use std::time::Duration;

pub fn wait_for_event(
    bus: &EventBus,
    predicate: impl Fn(&NodeEvent) -> bool,
    timeout: Duration,
) -> Option<NodeEvent> {
    let rx = bus.subscribe();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match rx.recv_timeout(remaining) {
            Ok(event) => {
                if predicate(&event) {
                    return Some(event);
                }
            }
            Err(_) => return None,
        }
    }
}

pub fn wait_for_block_applied(bus: &EventBus, timeout: Duration) -> Option<NodeEvent> {
    wait_for_event(bus, |e| matches!(e, NodeEvent::BlockApplied { .. }), timeout)
}
