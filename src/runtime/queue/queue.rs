use crate::io::log;
pub use crate::proto::payloads::{DispatchStrategy, QueueMode};
use crate::runtime::UuidV7;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ListenerRegistration {
    pub id: UuidV7,
    pub host: String,
    pub port: u16,
    pub failure_count: u64,
    pub unreachable_since: Option<Instant>,
    pub max_retries: u64,
    pub exhaustion_timeout: Duration,
}

impl ListenerRegistration {
    pub fn new(
        id: UuidV7,
        host: String,
        port: u16,
        max_retries: u64,
        exhaustion_timeout: Duration,
    ) -> Self {
        Self {
            id,
            host,
            port,
            failure_count: 0,
            unreachable_since: None,
            max_retries,
            exhaustion_timeout,
        }
    }
}

pub struct Queue {
    /// UUIDv7 keys are time-ordered, so iteration is FIFO by publish time.
    events: BTreeMap<UuidV7, Value>,
    /// Recipients are fixed when broadcast delivery first starts.
    pending_broadcasts: BTreeMap<UuidV7, BTreeSet<UuidV7>>,
    mode: QueueMode,
    dispatch_strategy: DispatchStrategy,
    listeners: Vec<ListenerRegistration>,
    round_robin_cursor: usize,
}

impl Queue {
    pub fn new(mode: QueueMode, dispatch_strategy: DispatchStrategy) -> Self {
        Self {
            events: BTreeMap::new(),
            pending_broadcasts: BTreeMap::new(),
            mode,
            dispatch_strategy,
            listeners: Vec::new(),
            round_robin_cursor: 0,
        }
    }

    pub fn mode(&self) -> QueueMode {
        self.mode
    }

    pub fn dispatch_strategy(&self) -> DispatchStrategy {
        self.dispatch_strategy
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

    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    pub fn listeners(&self) -> &[ListenerRegistration] {
        &self.listeners
    }

    pub fn listeners_mut(&mut self) -> &mut Vec<ListenerRegistration> {
        &mut self.listeners
    }

    pub fn attach_listener(&mut self, listener: ListenerRegistration) {
        self.listeners.push(listener);
    }

    pub fn detach_listener(&mut self, listener_id: &UuidV7) -> bool {
        if let Some(pos) = self.listeners.iter().position(|l| &l.id == listener_id) {
            self.listeners.remove(pos);
            let completed: Vec<_> = self
                .pending_broadcasts
                .iter_mut()
                .filter_map(|(event_id, pending)| {
                    pending.remove(listener_id);
                    pending.is_empty().then_some(*event_id)
                })
                .collect();
            for event_id in completed {
                self.remove_event(&event_id);
            }
            true
        } else {
            false
        }
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
            if let Some(value) = self.remove_event(&event_id) {
                result.push(value);
            }
        }
        log::info("consumed events");
        result
    }

    pub fn first_event(&self) -> Option<(UuidV7, Value)> {
        self.events.iter().next().map(|(&id, v)| (id, v.clone()))
    }

    pub fn remove_event(&mut self, event_id: &UuidV7) -> Option<Value> {
        self.pending_broadcasts.remove(event_id);
        self.events.remove(event_id)
    }

    pub fn broadcast_listeners(&mut self, event_id: UuidV7) -> Vec<ListenerRegistration> {
        let pending = self
            .pending_broadcasts
            .entry(event_id)
            .or_insert_with(|| self.listeners.iter().map(|l| l.id).collect());
        self.listeners
            .iter()
            .filter(|l| pending.contains(&l.id))
            .cloned()
            .collect()
    }

    pub fn acknowledge_broadcast(&mut self, event_id: &UuidV7, listener_id: &UuidV7) {
        if let Some(pending) = self.pending_broadcasts.get_mut(event_id) {
            pending.remove(listener_id);
            if pending.is_empty() {
                self.remove_event(event_id);
            }
        }
    }

    pub fn next_round_robin_listener(&mut self) -> Option<ListenerRegistration> {
        if self.listeners.is_empty() {
            return None;
        }
        let idx = self.round_robin_cursor % self.listeners.len();
        self.round_robin_cursor = (idx + 1) % self.listeners.len();
        Some(self.listeners[idx].clone())
    }
}
