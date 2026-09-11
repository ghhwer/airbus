use crate::io::log;
use crate::runtime::UuidV7;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct Queue {
    /// UUIDv7 keys are time-ordered, so iteration is FIFO by publish time.
    events: BTreeMap<UuidV7, Value>,
}

impl Queue {
    pub fn new(_config: Value) -> Self {
        Self {
            events: BTreeMap::new(),
        }
    }

    pub fn publish(&mut self, event_id: UuidV7, value: Value) -> Result<(), String> {
        if self.events.contains_key(&event_id) {
            return Err("Event ID already exists".to_string());
        }
        self.events.insert(event_id, value);
        log::info("published event");
        Ok(())
    }

    pub fn depth(&self) -> usize {
        self.events.len()
    }

    pub fn peek(&self, event_count: i64) -> Vec<(UuidV7, Value)> {
        let n = if event_count < 0 {
            0
        } else {
            event_count as usize
        };
        self.events
            .iter()
            .take(n)
            .map(|(&id, value)| (id, value.clone()))
            .collect()
    }

    pub fn consume(&mut self, event_count: i64) -> Vec<Value> {
        let n = if event_count < 0 {
            0
        } else {
            event_count as usize
        };
        let mut result = Vec::with_capacity(n);
        while result.len() < n {
            let Some((&event_id, _)) = self.events.iter().next() else {
                break;
            };
            if let Some(value) = self.events.remove(&event_id) {
                result.push(value);
            }
        }
        log::info("consumed events");
        result
    }
}
