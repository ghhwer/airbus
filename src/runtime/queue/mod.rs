mod manager;
#[allow(clippy::module_inception)]
mod queue;

pub use manager::QueueManager;
pub use queue::Queue;
