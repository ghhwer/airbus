//! Try round-robin candidates until one acknowledges, bounded by the initial listener count.
use super::{Event, Progress};
use crate::io::log;
use crate::runtime::queue::delivery::deliver_event;
use crate::runtime::queue::SharedQueue;
use std::time::Instant;

pub(super) fn deliver(
    name: &str,
    queue: &SharedQueue,
    event: &Event,
    max_attempts: usize,
) -> Progress {
    for _ in 0..max_attempts {
        let candidate = {
            let Ok(mut state) = queue.lock() else {
                return Progress::RetryLater;
            };
            state.next_round_robin_listener()
        };
        let Some(candidate) = candidate else {
            return Progress::RetryLater;
        };

        let delivered = deliver_event(&candidate, name, event.id, &event.value).is_ok();
        let Ok(mut state) = queue.lock() else {
            return Progress::RetryLater;
        };
        if delivered {
            state.remove_event(&event.id);
            log::info("dispatched event to worker listener");
        }
        state.record_delivery(name, &candidate.id, delivered, Instant::now());
        if delivered {
            return Progress::Delivered;
        }
    }
    Progress::RetryLater
}
