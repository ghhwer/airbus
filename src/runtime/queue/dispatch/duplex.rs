//! Deliver a full-duplex event to the opposite-side listener only.
use super::Progress;
use super::super::policy::Event;
use crate::io::log;
use crate::proto::payloads::DuplexSide;
use crate::runtime::queue::delivery::deliver_event;
use crate::runtime::queue::SharedQueue;
use std::time::Instant;

pub(super) fn deliver(
    name: &str,
    queue: &SharedQueue,
    event: &Event,
    target_side: DuplexSide,
) -> Progress {
    let candidate = {
        let Ok(state) = queue.lock() else {
            return Progress::RetryLater;
        };
        state.listener_for_side(target_side).cloned()
    };
    let Some(candidate) = candidate else {
        // Peer not attached yet — keep event buffered.
        return Progress::RetryLater;
    };

    let delivered = deliver_event(&candidate, name, event.id, &event.value).is_ok();
    let Ok(mut state) = queue.lock() else {
        return Progress::RetryLater;
    };
    if delivered {
        state.remove_event(&event.id);
        log::info("dispatched event to full-duplex peer listener");
    }
    state.record_delivery(name, &candidate.id, delivered, Instant::now());
    if delivered {
        Progress::Delivered
    } else {
        Progress::RetryLater
    }
}
