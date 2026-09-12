mod manager;
#[allow(clippy::module_inception)]
mod queue;

pub use manager::{ListenerInfo, QueueManager};
pub use queue::{DispatchStrategy, ListenerRegistration, Queue, QueueMode};
