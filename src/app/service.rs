use crate::proto::payloads::{
    AddParams, AddResult, EventObject, GetEventsParams, GetEventsResult, ListQueuesResult,
    ListQueuesResultQueuesItem, PeekEventsParams, PeekEventsResult, PeekEventsResultEventsItem,
    PeekEventsResultEventsItemId, PingResult, PostEventParams, PostEventResult, PostEventResultId,
    QueueName,
};
use crate::runtime::queue::QueueManager;
use crate::runtime::UuidV7;
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct InvalidParams {
    message: String,
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

    pub fn post_event(&self, params: PostEventParams) -> Result<PostEventResult, InvalidParams> {
        let id = UuidV7::generate();
        let event = Value::Object(params.event.0.clone());
        self.queue_manager
            .publish(params.queue.as_str(), id, event)
            .map_err(InvalidParams::new)?;
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
            .filter_map(|(name, depth)| {
                let name = QueueName::try_from(name).ok()?;
                Some(ListQueuesResultQueuesItem {
                    name,
                    depth: depth as u64,
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
