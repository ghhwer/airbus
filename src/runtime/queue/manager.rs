use super::Queue;
use crate::runtime::UuidV7;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct QueueManager {
    queues: Mutex<HashMap<String, Arc<Mutex<Queue>>>>,
}

impl QueueManager {
    pub fn new() -> Self {
        Self {
            queues: Mutex::new(HashMap::new()),
        }
    }

    fn ensure_queue(map: &mut HashMap<String, Arc<Mutex<Queue>>>, queue_name: &str) -> Arc<Mutex<Queue>> {
        map.entry(queue_name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(Queue::new(Value::Null))))
            .clone()
    }

    pub fn publish(&self, queue_name: &str, event_id: UuidV7, value: Value) -> Result<(), String> {
        let queue = {
            let mut map = self.queues.lock().map_err(|_| "queue manager poisoned")?;
            Self::ensure_queue(&mut map, queue_name)
        };
        let mut q = queue.lock().map_err(|_| "queue poisoned")?;
        q.publish(event_id, value)
    }

    pub fn consume(&self, queue_name: &str, event_count: i64) -> Vec<Value> {
        let queue = {
            let map = match self.queues.lock() {
                Ok(m) => m,
                Err(_) => return Vec::new(),
            };
            match map.get(queue_name) {
                Some(q) => q.clone(),
                None => return Vec::new(),
            }
        };
        let result = match queue.lock() {
            Ok(mut q) => q.consume(event_count),
            Err(_) => Vec::new(),
        };
        result
    }

    pub fn list(&self) -> Vec<(String, usize)> {
        let map = match self.queues.lock() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<(String, usize)> = map
            .iter()
            .filter_map(|(name, queue)| {
                let depth = queue.lock().ok()?.depth();
                Some((name.clone(), depth))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn peek(&self, queue_name: &str, event_count: i64) -> Vec<(UuidV7, Value)> {
        let queue = {
            let map = match self.queues.lock() {
                Ok(m) => m,
                Err(_) => return Vec::new(),
            };
            match map.get(queue_name) {
                Some(q) => q.clone(),
                None => return Vec::new(),
            }
        };
        let result = match queue.lock() {
            Ok(q) => q.peek(event_count),
            Err(_) => Vec::new(),
        };
        result
    }
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}
