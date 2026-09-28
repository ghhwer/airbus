//! Mode-specific publish / attach / delivery planning, chosen at queue creation.
use super::queue::opposite_side;
use super::{DispatchStrategy, ListenerRegistration, Queue, QueueMode};
use crate::io::log;
use crate::proto::payloads::DuplexSide;
use crate::runtime::UuidV7;
use serde_json::Value;

pub(super) struct Event {
    pub(super) id: UuidV7,
    pub(super) value: Value,
}

pub(super) enum Recipients {
    Broadcast(Vec<ListenerRegistration>),
    /// fifo (+ round_robin / single_node): compete for each event.
    Fifo { max_attempts: usize },
    Duplex { target_side: DuplexSide },
}

pub(super) struct DeliveryPlan {
    pub(super) event: Event,
    pub(super) recipients: Recipients,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModePolicy {
    Broadcast,
    Fifo { strategy: DispatchStrategy },
    FullDuplex,
}

impl ModePolicy {
    pub(super) fn new(mode: QueueMode, strategy: DispatchStrategy) -> Self {
        match mode {
            QueueMode::Broadcast => Self::Broadcast,
            QueueMode::Fifo => Self::Fifo { strategy },
            QueueMode::FullDuplex => Self::FullDuplex,
        }
    }

    pub(super) fn mode(&self) -> QueueMode {
        match self {
            Self::Broadcast => QueueMode::Broadcast,
            Self::Fifo { .. } => QueueMode::Fifo,
            Self::FullDuplex => QueueMode::FullDuplex,
        }
    }

    /// Queue already exists when this is called; mode decides further gates.
    pub(super) fn is_ready(&self, queue: &Queue) -> bool {
        match self {
            Self::Broadcast | Self::Fifo { .. } => true,
            Self::FullDuplex => {
                queue.listener_for_side(DuplexSide::Host).is_some()
                    && queue.listener_for_side(DuplexSide::Device).is_some()
            }
        }
    }

    pub(super) fn publish(
        &self,
        queue: &mut Queue,
        event_id: UuidV7,
        value: Value,
        side: Option<DuplexSide>,
    ) -> Result<(), String> {
        match self {
            Self::Broadcast | Self::Fifo { .. } => {
                if side.is_some() {
                    return Err(
                        "side is only valid when posting to a full-duplex queue".to_string(),
                    );
                }
                queue.insert_event(event_id, value)
            }
            Self::FullDuplex => {
                let publisher_side = side.ok_or_else(|| {
                    "side is required when posting to a full-duplex queue".to_string()
                })?;
                queue.insert_event(event_id, value)?;
                queue.set_duplex_target(event_id, opposite_side(publisher_side));
                Ok(())
            }
        }
    }

    pub(super) fn attach(
        &self,
        queue: &mut Queue,
        listener: ListenerRegistration,
    ) -> Result<(), String> {
        // Crash recovery: replace a stale registration before exclusivity checks.
        // - Exact host:port match (all modes).
        // - Same host, different port: only for exclusive slots (fifo single_node /
        //   full-duplex side), so broadcast / round_robin can still host many
        //   listeners on one machine.
        if let Some(stale_id) = Self::stale_listener_to_replace(self, queue, &listener) {
            log::info(&format!(
                "replacing listener {} with new registration {} at {}:{}",
                stale_id, listener.id, listener.host, listener.port
            ));
            queue.detach_listener(&stale_id);
        }

        match self {
            Self::FullDuplex => {
                let side = listener.side.ok_or_else(|| {
                    "side is required when attaching to a full-duplex queue".to_string()
                })?;
                // One listener per side (dispatch_strategy is rejected at create for duplex).
                if queue.listeners().iter().any(|l| l.side == Some(side)) {
                    return Err(format!("full-duplex side '{side}' already has a listener"));
                }
                if queue.listener_count() >= 2 {
                    return Err("full-duplex channel already has two listeners".to_string());
                }
            }
            Self::Fifo { strategy } => {
                if listener.side.is_some() {
                    return Err(
                        "side is only valid when attaching to a full-duplex queue".to_string(),
                    );
                }
                if *strategy == DispatchStrategy::SingleNode && queue.listener_count() > 0 {
                    return Err("fifo single_node queue already has a listener".to_string());
                }
            }
            Self::Broadcast => {
                if listener.side.is_some() {
                    return Err(
                        "side is only valid when attaching to a full-duplex queue".to_string(),
                    );
                }
            }
        }
        queue.push_listener(listener);
        Ok(())
    }

    /// Listener id to detach before attaching `listener`, if any.
    fn stale_listener_to_replace(
        policy: &Self,
        queue: &Queue,
        listener: &ListenerRegistration,
    ) -> Option<UuidV7> {
        if let Some(l) = queue
            .listeners()
            .iter()
            .find(|l| l.host == listener.host && l.port == listener.port)
        {
            return Some(l.id);
        }

        match policy {
            Self::Fifo {
                strategy: DispatchStrategy::SingleNode,
            } => queue
                .listeners()
                .iter()
                .find(|l| l.host == listener.host)
                .map(|l| l.id),
            Self::FullDuplex => {
                let side = listener.side?;
                queue
                    .listeners()
                    .iter()
                    .find(|l| l.side == Some(side) && l.host == listener.host)
                    .map(|l| l.id)
            }
            Self::Broadcast
            | Self::Fifo {
                strategy: DispatchStrategy::RoundRobin,
            } => None,
        }
    }

    pub(super) fn prepare_delivery(&self, queue: &mut Queue) -> Option<DeliveryPlan> {
        match self {
            Self::Broadcast => {
                if queue.listener_count() == 0 {
                    return None;
                }
                let (id, value) = queue.first_event()?;
                Some(DeliveryPlan {
                    event: Event { id, value },
                    recipients: Recipients::Broadcast(queue.broadcast_listeners(id)),
                })
            }
            Self::Fifo { .. } => {
                if queue.listener_count() == 0 {
                    return None;
                }
                let (id, value) = queue.first_event()?;
                Some(DeliveryPlan {
                    event: Event { id, value },
                    recipients: Recipients::Fifo {
                        max_attempts: queue.listener_count(),
                    },
                })
            }
            Self::FullDuplex => {
                let (id, value, target_side) = queue.first_duplex_event_ready()?;
                Some(DeliveryPlan {
                    event: Event { id, value },
                    recipients: Recipients::Duplex { target_side },
                })
            }
        }
    }
}
