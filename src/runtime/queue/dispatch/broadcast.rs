//! Deliver to every outstanding recipient; retain individual acknowledgments across retries.
use super::{Event, Progress};
use crate::runtime::queue::delivery::deliver_event;
use crate::runtime::queue::{ListenerRegistration, SharedQueue};
use std::time::Instant;

pub(super) fn deliver(
    name: &str,
    queue: &SharedQueue,
    event: &Event,
    listeners: &[ListenerRegistration],
) -> Progress {
    // Preserve the snapshot for this attempt, even if registrations change during I/O.
    let results: Vec<_> = listeners
        .iter()
        .map(|listener| {
            let result = deliver_event(listener, name, event.id, &event.value);
            (listener.id, result.is_ok())
        })
        .collect();
    let any_delivered = results.iter().any(|(_, delivered)| *delivered);

    let Ok(mut state) = queue.lock() else {
        return Progress::RetryLater;
    };
    let now = Instant::now();
    for (listener_id, delivered) in results {
        if delivered {
            state.acknowledge_broadcast(&event.id, &listener_id);
        }
        state.record_delivery(name, &listener_id, delivered, now);
    }

    if any_delivered {
        Progress::Delivered
    } else {
        Progress::RetryLater
    }
}
