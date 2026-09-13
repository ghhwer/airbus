mod delivery;
mod dispatch;
mod dispatcher;
mod listener;
mod manager;
mod policy;
#[allow(clippy::module_inception)]
mod queue;

pub use listener::ListenerRegistration;
pub use manager::{ListenerInfo, QueueManager};
pub use queue::{opposite_side, DispatchStrategy, Queue, QueueMode};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type SharedQueue = Arc<Mutex<Queue>>;
type QueueRegistry = Arc<Mutex<HashMap<String, SharedQueue>>>;
