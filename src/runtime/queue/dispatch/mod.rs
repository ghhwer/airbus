//! Snapshot one delivery under the queue lock, then run its policy without that lock.
mod broadcast;
mod worker;

use super::{ListenerRegistration, Queue, QueueMode, SharedQueue};
use crate::runtime::UuidV7;
use serde_json::Value;

struct Event {
    id: UuidV7,
    value: Value,
}

enum Recipients {
    Broadcast(Vec<ListenerRegistration>),
    Worker { max_attempts: usize },
}

struct DeliveryPlan {
    event: Event,
    recipients: Recipients,
}

impl DeliveryPlan {
    fn prepare(queue: &mut Queue) -> Option<Self> {
        if queue.listener_count() == 0 {
            return None;
        }
        let (id, value) = queue.first_event()?;
        let recipients = match queue.mode() {
            QueueMode::Broadcast => Recipients::Broadcast(queue.broadcast_listeners(id)),
            QueueMode::Worker => Recipients::Worker {
                max_attempts: queue.listener_count(),
            },
        };
        Some(Self {
            event: Event { id, value },
            recipients,
        })
    }
}

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
            DeliveryPlan::prepare(&mut state)
        };
        let Some(plan) = plan else { return };
        let progress = match plan.recipients {
            Recipients::Broadcast(listeners) => {
                broadcast::deliver(name, queue, &plan.event, &listeners)
            }
            Recipients::Worker { max_attempts } => {
                worker::deliver(name, queue, &plan.event, max_attempts)
            }
        };
        if matches!(progress, Progress::RetryLater) {
            return;
        }
    }
}
