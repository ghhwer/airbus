mod delivery;
mod dispatch;
mod dispatcher;
mod listener;
mod manager;
#[allow(clippy::module_inception)]
mod queue;

pub use listener::ListenerRegistration;
pub use manager::{ListenerInfo, QueueManager};
pub use queue::{DispatchStrategy, Queue, QueueMode};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type SharedQueue = Arc<Mutex<Queue>>;
type QueueRegistry = Arc<Mutex<HashMap<String, SharedQueue>>>;
