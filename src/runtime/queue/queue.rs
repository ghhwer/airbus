use super::policy::{DeliveryPlan, ModePolicy};
use super::ListenerRegistration;
use crate::io::log;
pub use crate::proto::payloads::{DispatchStrategy, DuplexSide, QueueMode};
use crate::runtime::UuidV7;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

pub fn opposite_side(side: DuplexSide) -> DuplexSide {
    match side {
        DuplexSide::Host => DuplexSide::Device,
        DuplexSide::Device => DuplexSide::Host,
    }
}

pub struct Queue {
    /// UUIDv7 keys are time-ordered, so iteration is FIFO by publish time.
    events: BTreeMap<UuidV7, Value>,
    /// Full-duplex: target listener side for each buffered event.
    duplex_targets: BTreeMap<UuidV7, DuplexSide>,
    /// Recipients are fixed when broadcast delivery first starts.
    pending_broadcasts: BTreeMap<UuidV7, BTreeSet<UuidV7>>,
    /// Tracks broadcast events that received at least one acknowledgment.
    acknowledged_events: BTreeSet<UuidV7>,
    policy: ModePolicy,
    /// Retained from create_queue for introspection; fifo exclusivity lives on policy.
    dispatch_strategy: DispatchStrategy,
    listeners: Vec<ListenerRegistration>,
    round_robin_cursor: usize,
}

impl Queue {
    pub fn new(mode: QueueMode, dispatch_strategy: DispatchStrategy) -> Self {
        Self {
            events: BTreeMap::new(),
            duplex_targets: BTreeMap::new(),
            pending_broadcasts: BTreeMap::new(),
            acknowledged_events: BTreeSet::new(),
            policy: ModePolicy::new(mode, dispatch_strategy),
            dispatch_strategy,
            listeners: Vec::new(),
            round_robin_cursor: 0,
        }
    }

    pub fn mode(&self) -> QueueMode {
        self.policy.mode()
    }

    pub fn dispatch_strategy(&self) -> DispatchStrategy {
        self.dispatch_strategy
    }

    pub fn publish(
        &mut self,
        event_id: UuidV7,
        value: Value,
        side: Option<DuplexSide>,
    ) -> Result<(), String> {
        let policy = self.policy;
        policy.publish(self, event_id, value, side)
    }

    pub(super) fn insert_event(&mut self, event_id: UuidV7, value: Value) -> Result<(), String> {
        if self.events.contains_key(&event_id) {
            return Err("Event ID already exists".to_string());
        }
        self.events.insert(event_id, value);
        log::info("published event");
        Ok(())
    }

    pub(super) fn set_duplex_target(&mut self, event_id: UuidV7, target: DuplexSide) {
        self.duplex_targets.insert(event_id, target);
    }

    pub(super) fn push_listener(&mut self, listener: ListenerRegistration) {
        self.listeners.push(listener);
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

    pub fn listener_for_side(&self, side: DuplexSide) -> Option<&ListenerRegistration> {
        self.listeners.iter().find(|l| l.side == Some(side))
    }

    pub(super) fn record_delivery(
        &mut self,
        queue_name: &str,
        listener_id: &UuidV7,
        delivered: bool,
        now: Instant,
    ) {
        let Some(listener) = self.listeners.iter_mut().find(|l| &l.id == listener_id) else {
            return;
        };
        if listener.record_delivery(delivered, now) {
            log::warn(&format!(
                "client unreachable exhaustion: evicted listener {} on {}:{} for queue '{}' (retries={})",
                listener.id, listener.host, listener.port, queue_name, listener.failure_count
            ));
            self.detach_listener(listener_id);
        }
    }

    pub fn attach_listener(&mut self, listener: ListenerRegistration) -> Result<(), String> {
        let policy = self.policy;
        policy.attach(self, listener)
    }

    pub fn detach_listener(&mut self, listener_id: &UuidV7) -> bool {
        if let Some(pos) = self.listeners.iter().position(|l| &l.id == listener_id) {
            self.listeners.remove(pos);
            let mut completed = Vec::new();
            let mut unacknowledged_exhausted = Vec::new();
            for (event_id, pending) in self.pending_broadcasts.iter_mut() {
                pending.remove(listener_id);
                if pending.is_empty() {
                    if self.acknowledged_events.contains(event_id) {
                        completed.push(*event_id);
                    } else {
                        unacknowledged_exhausted.push(*event_id);
                    }
                }
            }
            for event_id in completed {
                self.remove_event(&event_id);
            }
            for event_id in unacknowledged_exhausted {
                self.pending_broadcasts.remove(&event_id);
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

    pub fn first_event(&self) -> Option<(UuidV7, Value)> {
        self.events.iter().next().map(|(&id, v)| (id, v.clone()))
    }

    pub fn first_duplex_event_ready(&self) -> Option<(UuidV7, Value, DuplexSide)> {
        for (&id, value) in &self.events {
            let Some(&target) = self.duplex_targets.get(&id) else {
                continue;
            };
            if self.listener_for_side(target).is_some() {
                return Some((id, value.clone(), target));
            }
        }
        None
    }

    pub fn remove_event(&mut self, event_id: &UuidV7) -> Option<Value> {
        self.pending_broadcasts.remove(event_id);
        self.acknowledged_events.remove(event_id);
        self.duplex_targets.remove(event_id);
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
            self.acknowledged_events.insert(*event_id);
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

    pub(super) fn prepare_delivery(&mut self) -> Option<DeliveryPlan> {
        let policy = self.policy;
        policy.prepare_delivery(self)
    }
}
