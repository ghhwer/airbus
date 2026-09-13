//! Snapshot one delivery under the queue lock, then run its policy without that lock.
mod broadcast;
mod duplex;
mod worker;

use super::policy::Recipients;
use super::SharedQueue;

/// RetryLater yields to the scheduler instead of spinning on unreachable listeners.
enum Progress {
    /// At least one recipient acknowledged; the queue can make another attempt.
    Delivered,
    RetryLater,
}

pub(super) fn dispatch_queue(name: &str, queue: &SharedQueue) {
    loop {
        let plan = {
            let Ok(mut state) = queue.lock() else { return };
            state.prepare_delivery()
        };
        let Some(plan) = plan else { return };
        let progress = match plan.recipients {
            Recipients::Broadcast(listeners) => {
                broadcast::deliver(name, queue, &plan.event, &listeners)
            }
            Recipients::Fifo { max_attempts } => {
                worker::deliver(name, queue, &plan.event, max_attempts)
            }
            Recipients::Duplex { target_side } => {
                duplex::deliver(name, queue, &plan.event, target_side)
            }
        };
        if matches!(progress, Progress::RetryLater) {
            return;
        }
    }
}
