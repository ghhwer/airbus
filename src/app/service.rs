use crate::proto::payloads::{
    AddParams, AddResult, AttachListenerParams, AttachListenerResult,
    AttachListenerResultListenerId, CreateQueueParams, CreateQueueResult, DetachListenerParams,
    DetachListenerResult, DetachListenerResultListenerId, DispatchStrategy, EventObject,
    GetEventsParams, GetEventsResult, ListListenersParams, ListListenersResult,
    ListListenersResultListenersItem, ListListenersResultListenersItemId, ListQueuesResult,
    ListQueuesResultQueuesItem, PeekEventsParams, PeekEventsResult, PeekEventsResultEventsItem,
    PeekEventsResultEventsItemId, PingResult, PostEventParams, PostEventResult, PostEventResultId,
    QueueMode, QueueName,
};
use crate::runtime::queue::QueueManager;
use crate::runtime::UuidV7;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct InvalidParams {
    pub message: String,
}

impl InvalidParams {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for InvalidParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InvalidParams {}

/// Deserialize JSON-RPC `params` into a generated payload type.
pub fn decode_params<T: DeserializeOwned>(params: &Value) -> Result<T, InvalidParams> {
    serde_json::from_value(params.clone()).map_err(|e| InvalidParams::new(e.to_string()))
}

/// Application methods only. Sockets live in `io`; JSON-RPC documents in `proto`.
pub struct AppService {
    queue_manager: QueueManager,
}

impl AppService {
    pub fn new() -> Self {
        Self {
            queue_manager: QueueManager::new(),
        }
    }

    pub fn ping(&self) -> PingResult {
        PingResult("pong".into())
    }

    pub fn add(&self, params: AddParams) -> AddResult {
        AddResult(params.0[0] + params.0[1])
    }

    pub fn create_queue(
        &self,
        params: CreateQueueParams,
    ) -> Result<CreateQueueResult, InvalidParams> {
        let mode = params.mode.unwrap_or(QueueMode::Broadcast);
        let strategy = params
            .dispatch_strategy
            .unwrap_or(DispatchStrategy::RoundRobin);

        let created = self
            .queue_manager
            .create_queue(params.queue.as_str(), mode, strategy)
            .map_err(InvalidParams::new)?;

        let mode = self
            .queue_manager
            .get_queue_mode(params.queue.as_str())
            .ok_or_else(|| InvalidParams::new("Queue does not exist"))?;

        Ok(CreateQueueResult {
            created,
            mode,
            queue: params.queue,
        })
    }

    pub fn post_event(&self, params: PostEventParams) -> Result<PostEventResult, InvalidParams> {
        let id = UuidV7::generate();
        let event = Value::Object(params.event.0.clone());
        self.queue_manager
            .publish(params.queue.as_str(), id, event)
            .map_err(|_| {
                InvalidParams::new(format!("Queue '{}' does not exist", params.queue.as_str()))
            })?;
        Ok(PostEventResult {
            id: PostEventResultId::try_from(id.to_string())
                .map_err(|e| InvalidParams::new(e.to_string()))?,
            queue: params.queue,
        })
    }

    pub fn get_events(&self, params: GetEventsParams) -> GetEventsResult {
        let events = self
            .queue_manager
            .consume(params.queue.as_str(), params.count)
            .into_iter()
            .map(value_as_event_object)
            .collect();
        GetEventsResult {
            queue: params.queue,
            events,
        }
    }

    pub fn list_queues(&self) -> ListQueuesResult {
        let queues = self
            .queue_manager
            .list()
            .into_iter()
            .filter_map(|(name, depth, mode, listener_count)| {
                let name = QueueName::try_from(name).ok()?;
                Some(ListQueuesResultQueuesItem {
                    depth: depth as u64,
                    listener_count: listener_count as u64,
                    mode,
                    name,
                })
            })
            .collect();
        ListQueuesResult { queues }
    }

    pub fn peek_events(&self, params: PeekEventsParams) -> Result<PeekEventsResult, InvalidParams> {
        let events = self
            .queue_manager
            .peek(params.queue.as_str(), params.count)
            .into_iter()
            .map(|(id, value)| {
                Ok(PeekEventsResultEventsItem {
                    id: PeekEventsResultEventsItemId::try_from(id.to_string())
                        .map_err(|e| InvalidParams::new(e.to_string()))?,
                    event: value_as_event_object(value),
                })
            })
            .collect::<Result<Vec<_>, InvalidParams>>()?;
        Ok(PeekEventsResult {
            queue: params.queue,
            events,
        })
    }

    pub fn attach_listener(
        &self,
        params: AttachListenerParams,
    ) -> Result<AttachListenerResult, InvalidParams> {
        let listener_id = UuidV7::generate();
        let host = params.host.unwrap_or_else(|| "127.0.0.1".to_string());
        let port = u16::try_from(params.port.get())
            .map_err(|_| InvalidParams::new("port must be between 1 and 65535"))?;
        let max_retries = params.max_retries.unwrap_or(3);
        let exhaustion_timeout =
            Duration::from_millis(params.exhaustion_timeout_ms.unwrap_or(10000));

        self.queue_manager
            .attach_listener(
                params.queue.as_str(),
                listener_id,
                host,
                port,
                max_retries,
                exhaustion_timeout,
            )
            .map_err(|_| {
                InvalidParams::new(format!("Queue '{}' does not exist", params.queue.as_str()))
            })?;

        let id_typed = AttachListenerResultListenerId::try_from(listener_id.to_string().as_str())
            .map_err(|e| InvalidParams::new(e.to_string()))?;

        Ok(AttachListenerResult {
            listener_id: id_typed,
            queue: params.queue,
            status: "attached".to_string(),
        })
    }

    pub fn detach_listener(
        &self,
        params: DetachListenerParams,
    ) -> Result<DetachListenerResult, InvalidParams> {
        let listener_id: UuidV7 = params
            .listener_id
            .as_str()
            .parse()
            .map_err(|e: String| InvalidParams::new(e))?;
        let detached = self.queue_manager.detach_listener(&listener_id);
        let id_typed = DetachListenerResultListenerId::try_from(params.listener_id.as_str())
            .map_err(|e| InvalidParams::new(e.to_string()))?;
        Ok(DetachListenerResult {
            detached,
            listener_id: id_typed,
        })
    }

    pub fn list_listeners(
        &self,
        params: Option<ListListenersParams>,
    ) -> Result<ListListenersResult, InvalidParams> {
        let filter_queue = params
            .as_ref()
            .and_then(|p| p.queue.as_ref())
            .map(|q| q.as_str());
        let list = self.queue_manager.list_listeners(filter_queue);
        let listeners = list
            .into_iter()
            .map(|item| {
                let id = ListListenersResultListenersItemId::try_from(item.id.to_string().as_str())
                    .map_err(|e| InvalidParams::new(e.to_string()))?;
                let queue = QueueName::try_from(item.queue)
                    .map_err(|e| InvalidParams::new(e.to_string()))?;
                let port = std::num::NonZeroU64::new(item.port as u64)
                    .ok_or_else(|| InvalidParams::new("port is zero"))?;
                Ok(ListListenersResultListenersItem {
                    active: item.active,
                    failure_count: item.failure_count,
                    host: item.host,
                    id,
                    mode: item.mode,
                    port,
                    queue,
                })
            })
            .collect::<Result<Vec<_>, InvalidParams>>()?;
        Ok(ListListenersResult { listeners })
    }
}

fn value_as_event_object(value: Value) -> EventObject {
    match value {
        Value::Object(map) => EventObject(map),
        other => {
            let mut map = serde_json::Map::new();
            map.insert("_".into(), other);
            EventObject(map)
        }
    }
}

impl Default for AppService {
    fn default() -> Self {
        Self::new()
    }
}

/// Helpers for constructing typed params in tests / callers.
pub fn queue_name(s: impl Into<String>) -> Result<QueueName, InvalidParams> {
    QueueName::try_from(s.into()).map_err(|e| InvalidParams::new(e.to_string()))
}

pub fn event_object(value: Value) -> Result<EventObject, InvalidParams> {
    match value {
        Value::Object(map) => Ok(EventObject(map)),
        _ => Err(InvalidParams::new("event must be an object")),
    }
}
