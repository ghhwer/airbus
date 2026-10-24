//! JSON-RPC envelopes and typed payload codecs; no sockets or tasks.
//!
//! Runtime JSON Schema validation lives on the daemon. Clients encode/decode
//! generated payload types and check JSON-RPC envelopes only.

use crate::payloads::{ListenerEventParams, ListenerEventResult, ListenerEventResultStatus};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use thiserror::Error;

pub const VERSION: &str = "2.0";

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

#[derive(Debug, Error, Clone)]
#[error("{code}: {message}")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
    pub id: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
            id: None,
        }
    }

    pub fn with_id(mut self, id: Option<Value>) -> Self {
        self.id = id;
        self
    }
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error(transparent)]
    Rpc(#[from] RpcError),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn valid_id(value: &Value) -> bool {
    value.is_null() || value.is_string() || value.is_number()
}

/// Build a JSON-RPC request. Pass `id = None` for a notification.
pub fn request(method: &str, params: Option<Value>, id: Option<Value>) -> Value {
    let mut document = Map::new();
    document.insert("jsonrpc".into(), Value::String(VERSION.into()));
    document.insert("method".into(), Value::String(method.into()));
    if let Some(id) = id {
        document.insert("id".into(), id);
    }
    if let Some(params) = params {
        document.insert("params".into(), params);
    }
    Value::Object(document)
}

pub fn to_wire<T: Serialize>(value: &T) -> Result<Value, ProtocolError> {
    Ok(serde_json::to_value(value)?)
}

pub fn validate_response(response: &Value, expected_id: &Value) -> Result<Value, ProtocolError> {
    let obj = response
        .as_object()
        .ok_or_else(|| ProtocolError::Invalid("invalid JSON-RPC response version".into()))?;
    if obj.get("jsonrpc").and_then(Value::as_str) != Some(VERSION) {
        return Err(ProtocolError::Invalid(
            "invalid JSON-RPC response version".into(),
        ));
    }
    let id = obj
        .get("id")
        .ok_or_else(|| ProtocolError::Invalid("invalid JSON-RPC response id".into()))?;
    if !valid_id(id) {
        return Err(ProtocolError::Invalid(
            "invalid JSON-RPC response id".into(),
        ));
    }
    if id != expected_id {
        return Err(ProtocolError::Invalid(
            "JSON-RPC response id does not match request".into(),
        ));
    }
    let has_result = obj.contains_key("result");
    let has_error = obj.contains_key("error");
    if has_result == has_error {
        return Err(ProtocolError::Invalid(
            "response must contain exactly one of result or error".into(),
        ));
    }
    if has_error {
        let error = obj
            .get("error")
            .and_then(Value::as_object)
            .ok_or_else(|| ProtocolError::Invalid("invalid JSON-RPC error object".into()))?;
        if error.get("code").and_then(Value::as_i64).is_none()
            || error.get("message").and_then(Value::as_str).is_none()
        {
            return Err(ProtocolError::Invalid(
                "invalid JSON-RPC error object".into(),
            ));
        }
    }
    Ok(response.clone())
}

pub fn response_result(response: &Value) -> Result<Value, RpcError> {
    let obj = response
        .as_object()
        .expect("validate_response ensures object");
    if let Some(error) = obj.get("error") {
        let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let data = error.get("data").cloned();
        return Err(RpcError {
            code,
            message,
            data,
            id: obj.get("id").cloned(),
        });
    }
    Ok(obj
        .get("result")
        .cloned()
        .expect("validate_response ensures result or error"))
}

pub fn decode_result<T: DeserializeOwned>(value: Value) -> Result<T, ProtocolError> {
    Ok(serde_json::from_value(value)?)
}

/// Correlate batch responses to requests (notifications map to `None`).
pub fn batch_responses(
    requests: &[Value],
    response: Option<&Value>,
) -> Result<Vec<Option<Value>>, ProtocolError> {
    let expected: Vec<&Value> = requests
        .iter()
        .filter_map(|item| item.get("id"))
        .collect();
    if expected.is_empty() {
        if response.is_some() {
            return Err(ProtocolError::Invalid(
                "notifications must not receive a response".into(),
            ));
        }
        return Ok(vec![None; requests.len()]);
    }
    let response = response.ok_or_else(|| {
        ProtocolError::Invalid("batch response must be an array".into())
    })?;
    let items = response
        .as_array()
        .ok_or_else(|| ProtocolError::Invalid("batch response must be an array".into()))?;
    let mut by_id: HashMap<String, Value> = HashMap::new();
    let expected_keys: std::collections::HashSet<String> =
        expected.iter().map(|id| id.to_string()).collect();
    for item in items {
        let id = item
            .get("id")
            .ok_or_else(|| ProtocolError::Invalid("invalid batch response".into()))?;
        if !valid_id(id) {
            return Err(ProtocolError::Invalid("invalid batch response".into()));
        }
        let key = id.to_string();
        if !expected_keys.contains(&key) || by_id.contains_key(&key) {
            return Err(ProtocolError::Invalid(
                "unexpected or duplicate batch response id".into(),
            ));
        }
        by_id.insert(key, validate_response(item, id)?);
    }
    if by_id.len() != expected_keys.len() {
        return Err(ProtocolError::Invalid("missing batch response".into()));
    }
    Ok(requests
        .iter()
        .map(|item| {
            item.get("id")
                .map(|id| by_id.get(&id.to_string()).cloned().unwrap())
        })
        .collect())
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": VERSION,
        "id": id,
        "error": { "code": code, "message": message },
    })
}

/// Decoded inbound `on_event` ready for an async callback.
pub struct DecodedOnEvent {
    pub id: Option<Value>,
    pub event: Value,
}

pub enum OnEventDecode {
    /// Valid `on_event`; caller should invoke the callback then [`event_ack`] / [`event_error`].
    Event(DecodedOnEvent),
    /// Immediate JSON-RPC response (parse/envelope error), or `None` for ignored notification.
    Reply(Option<Value>),
}

/// Decode an inbound listener request without invoking a callback.
pub fn decode_on_event(body: &str) -> OnEventDecode {
    let document: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            return OnEventDecode::Reply(Some(error_response(
                Value::Null,
                PARSE_ERROR,
                "invalid JSON",
            )))
        }
    };
    let obj = match document.as_object() {
        Some(o) => o,
        None => {
            return OnEventDecode::Reply(Some(error_response(
                Value::Null,
                INVALID_REQUEST,
                "invalid request",
            )))
        }
    };
    let id = obj.get("id").cloned();
    let id_for_error = match &id {
        Some(v) if valid_id(v) => v.clone(),
        Some(_) => Value::Null,
        None => Value::Null,
    };
    if obj.get("jsonrpc").and_then(Value::as_str) != Some(VERSION)
        || !obj
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(|m| !m.is_empty())
        || (id.is_some() && !valid_id(id.as_ref().unwrap()))
    {
        return OnEventDecode::Reply(Some(error_response(
            id_for_error,
            INVALID_REQUEST,
            "invalid request",
        )));
    }
    let notification = id.is_none();
    let method = obj.get("method").and_then(Value::as_str).unwrap_or("");
    if method != "on_event" {
        return OnEventDecode::Reply(if notification {
            None
        } else {
            Some(error_response(
                id.unwrap_or(Value::Null),
                METHOD_NOT_FOUND,
                "method not found",
            ))
        });
    }
    let params = match obj.get("params") {
        Some(p) => p.clone(),
        None => {
            return OnEventDecode::Reply(if notification {
                None
            } else {
                Some(error_response(
                    id.unwrap_or(Value::Null),
                    INVALID_PARAMS,
                    "missing params",
                ))
            });
        }
    };
    let event: ListenerEventParams = match serde_json::from_value(params) {
        Ok(e) => e,
        Err(err) => {
            return OnEventDecode::Reply(if notification {
                None
            } else {
                Some(error_response(
                    id.unwrap_or(Value::Null),
                    INVALID_PARAMS,
                    &err.to_string(),
                ))
            });
        }
    };
    let wire = match serde_json::to_value(&event) {
        Ok(v) => v,
        Err(err) => {
            return OnEventDecode::Reply(if notification {
                None
            } else {
                Some(error_response(
                    id.unwrap_or(Value::Null),
                    INTERNAL_ERROR,
                    &err.to_string(),
                ))
            });
        }
    };
    OnEventDecode::Event(DecodedOnEvent { id, event: wire })
}

pub fn event_ack(id: Value) -> Value {
    let acknowledgment = ListenerEventResult {
        status: ListenerEventResultStatus::Ok,
    };
    json!({
        "jsonrpc": VERSION,
        "id": id,
        "result": acknowledgment,
    })
}

pub fn event_error(id: Value, code: i64, message: &str) -> Value {
    error_response(id, code, message)
}

/// Handle an inbound `on_event` document with a sync callback (tests / simple listeners).
pub fn handle_event(
    body: &str,
    callback: impl FnOnce(Value) -> Result<(), String>,
) -> Option<Value> {
    match decode_on_event(body) {
        OnEventDecode::Reply(reply) => reply,
        OnEventDecode::Event(DecodedOnEvent { id, event }) => {
            let notification = id.is_none();
            if let Err(err) = callback(event) {
                return if notification {
                    None
                } else {
                    Some(error_response(
                        id.unwrap_or(Value::Null),
                        INTERNAL_ERROR,
                        &err,
                    ))
                };
            }
            if notification {
                None
            } else {
                Some(event_ack(id.unwrap_or(Value::Null)))
            }
        }
    }
}
