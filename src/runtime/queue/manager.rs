use super::dispatcher::Dispatcher;
use super::{DispatchStrategy, ListenerRegistration, Queue, QueueMode, QueueRegistry, SharedQueue};
use crate::proto::payloads::DuplexSide;
use crate::runtime::UuidV7;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ListenerInfo {
    pub id: UuidV7,
    pub queue: String,
    pub host: String,
    pub port: u16,
    pub mode: QueueMode,
    pub failure_count: u64,
    pub active: bool,
    pub side: Option<DuplexSide>,
}

/// Owns the queue registry; delivery and background scheduling live in Dispatcher.
pub struct QueueManager {
    queues: QueueRegistry,
    dispatcher: Dispatcher,
}

impl QueueManager {
    pub fn new() -> Self {
        let queues = Arc::new(Mutex::new(HashMap::new()));
        let dispatcher = Dispatcher::start(Arc::clone(&queues));
        Self { queues, dispatcher }
    }

    fn find_queue(&self, queue_name: &str) -> Result<SharedQueue, String> {
        let queues = self.queues.lock().map_err(|_| "queue manager poisoned")?;
        queues
            .get(queue_name)
            .cloned()
            .ok_or_else(|| format!("Queue not found: {queue_name}"))
    }

    pub fn create_queue(
        &self,
        queue_name: &str,
        mode: QueueMode,
        strategy: DispatchStrategy,
    ) -> Result<bool, String> {
        let mut map = self.queues.lock().map_err(|_| "queue manager poisoned")?;
        if map.contains_key(queue_name) {
            Ok(false)
        } else {
            map.insert(
                queue_name.to_string(),
                Arc::new(Mutex::new(Queue::new(mode, strategy))),
            );
            Ok(true)
        }
    }

    pub fn get_queue_mode(&self, queue_name: &str) -> Option<QueueMode> {
        let q = self.find_queue(queue_name).ok()?;
        let guard = q.lock().ok()?;
        Some(guard.mode())
    }

    /// Mode-aware readiness: missing → false; otherwise delegates to the queue.
    pub fn is_ready(&self, queue_name: &str) -> bool {
        let Ok(queue) = self.find_queue(queue_name) else {
            return false;
        };
        let Ok(q) = queue.lock() else {
            return false;
        };
        q.is_ready()
    }

    pub fn publish(
        &self,
        queue_name: &str,
        event_id: UuidV7,
        value: Value,
        side: Option<DuplexSide>,
    ) -> Result<(), String> {
        let queue = self.find_queue(queue_name)?;
        {
            let mut q = queue.lock().map_err(|_| "queue poisoned")?;
            q.publish(event_id, value, side)?;
        }
        self.dispatcher.wake();
        Ok(())
    }

    pub fn list(&self) -> Vec<(String, usize, QueueMode, usize)> {
        let map = match self.queues.lock() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<(String, usize, QueueMode, usize)> = map
            .iter()
            .filter_map(|(name, queue)| {
                let q = queue.lock().ok()?;
                Some((name.clone(), q.depth(), q.mode(), q.listener_count()))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn peek(&self, queue_name: &str, event_count: i64) -> Vec<(UuidV7, Value)> {
        let Ok(queue) = self.find_queue(queue_name) else {
            return Vec::new();
        };
        let result = match queue.lock() {
            Ok(q) => q.peek(event_count),
            Err(_) => Vec::new(),
        };
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub fn attach_listener(
        &self,
        queue_name: &str,
        listener_id: UuidV7,
        host: String,
        port: u16,
        max_retries: u64,
        exhaustion_timeout: Duration,
        side: Option<DuplexSide>,
    ) -> Result<(), String> {
        let queue = self.find_queue(queue_name)?;
        {
            let mut q = queue.lock().map_err(|_| "queue poisoned")?;
            q.attach_listener(ListenerRegistration::new(
                listener_id,
                host,
                port,
                max_retries,
                exhaustion_timeout,
                side,
            ))?;
        }
        self.dispatcher.wake();
        Ok(())
    }

    pub fn detach_listener(&self, listener_id: &UuidV7) -> bool {
        let map = match self.queues.lock() {
            Ok(m) => m,
            Err(_) => return false,
        };
        for queue in map.values() {
            if let Ok(mut q) = queue.lock() {
                if q.detach_listener(listener_id) {
                    return true;
                }
            }
        }
        false
    }

    pub fn list_listeners(&self, filter_queue: Option<&str>) -> Vec<ListenerInfo> {
        let map = match self.queues.lock() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for (name, queue) in map.iter() {
            if let Some(fq) = filter_queue {
                if name != fq {
                    continue;
                }
            }
            if let Ok(q) = queue.lock() {
                let mode = q.mode();
                for l in q.listeners() {
                    out.push(ListenerInfo {
                        id: l.id,
                        queue: name.clone(),
                        host: l.host.clone(),
                        port: l.port,
                        mode,
                        failure_count: l.failure_count,
                        active: l.failure_count == 0,
                        side: l.side,
                    });
                }
            }
        }
        out.sort_by_key(|a| a.id);
        out
    }
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}
