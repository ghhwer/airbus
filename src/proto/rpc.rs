//! JSON-RPC documents (requests, responses, errors). No sockets or fds.

use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub const VERSION: &str = "2.0";

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

#[derive(Debug, Clone)]
pub struct Error {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl Error {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i64, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(PARSE_ERROR, message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message)
    }

    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self::with_data(METHOD_NOT_FOUND, "Method not found", Value::String(method.into()))
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(INTERNAL_ERROR, message)
    }

    pub fn to_json(&self) -> Value {
        let mut obj = Map::new();
        obj.insert("code".into(), json!(self.code));
        obj.insert("message".into(), json!(self.message));
        if let Some(data) = &self.data {
            obj.insert("data".into(), data.clone());
        }
        Value::Object(obj)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

pub fn request(method: impl Into<String>, id: Value) -> Value {
    json!({
        "jsonrpc": VERSION,
        "method": method.into(),
        "id": id,
    })
}

pub fn request_with_params(method: impl Into<String>, params: Value, id: Value) -> Value {
    let mut obj = request(method, id);
    if let Some(map) = obj.as_object_mut() {
        map.insert("params".into(), params);
    }
    obj
}

pub fn notification(method: impl Into<String>) -> Value {
    json!({
        "jsonrpc": VERSION,
        "method": method.into(),
    })
}

pub fn notification_with_params(method: impl Into<String>, params: Value) -> Value {
    let mut obj = notification(method);
    if let Some(map) = obj.as_object_mut() {
        map.insert("params".into(), params);
    }
    obj
}

fn error_object(err: &Error) -> Value {
    err.to_json()
}

fn result_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": VERSION,
        "result": result,
        "id": id,
    })
}

fn error_response(id: Value, err: &Error) -> Value {
    json!({
        "jsonrpc": VERSION,
        "error": error_object(err),
        "id": id,
    })
}

fn valid_id(id: &Value) -> bool {
    id.is_null() || id.is_string() || id.is_number()
}

fn valid_params(params: &Value) -> bool {
    params.is_array() || params.is_object() || params.is_null()
}

type Handler = Box<dyn Fn(Value) -> Result<Value, Error> + Send + Sync>;

pub struct Server {
    handlers: HashMap<String, Handler>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn add<F>(&mut self, method: impl Into<String>, handler: F)
    where
        F: Fn(Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.handlers.insert(method.into(), Box::new(handler));
    }

    fn handle_one(&self, value: &Value) -> Option<Value> {
        let obj = match value.as_object() {
            Some(o) => o,
            None => return Some(error_response(Value::Null, &Error::invalid_request("Invalid Request"))),
        };

        let version_ok = obj
            .get("jsonrpc")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == VERSION);
        if !version_ok {
            let mut id = obj.get("id").cloned().unwrap_or(Value::Null);
            if !valid_id(&id) {
                id = Value::Null;
            }
            return Some(error_response(
                id,
                &Error::invalid_request("jsonrpc must be \"2.0\""),
            ));
        }

        let is_notification = !obj.contains_key("id");
        let id = if is_notification {
            Value::Null
        } else {
            obj.get("id").cloned().unwrap_or(Value::Null)
        };
        if !is_notification && !valid_id(&id) {
            return Some(error_response(
                Value::Null,
                &Error::invalid_request("id must be string, number, or null"),
            ));
        }

        let method = match obj.get("method").and_then(|v| v.as_str()) {
            Some(m) => m.to_string(),
            None => {
                if is_notification {
                    return None;
                }
                return Some(error_response(
                    id,
                    &Error::invalid_request("method must be a string"),
                ));
            }
        };

        let params = match obj.get("params") {
            Some(p) => {
                if !valid_params(p) {
                    if is_notification {
                        return None;
                    }
                    return Some(error_response(
                        id,
                        &Error::invalid_params("params must be array or object"),
                    ));
                }
                p.clone()
            }
            None => Value::Null,
        };

        let Some(handler) = self.handlers.get(&method) else {
            if is_notification {
                return None;
            }
            return Some(error_response(id, &Error::method_not_found(method)));
        };

        match handler(params) {
            Ok(result) => {
                if is_notification {
                    None
                } else {
                    Some(result_response(id, result))
                }
            }
            Err(err) => {
                if is_notification {
                    None
                } else {
                    Some(error_response(id, &err))
                }
            }
        }
    }

    /// Returns a JSON-RPC response document, or empty if only notifications.
    pub fn handle(&self, message: &str) -> String {
        let doc: Value = match serde_json::from_str(message) {
            Ok(v) => v,
            Err(e) => {
                return error_response(Value::Null, &Error::parse_error(e.to_string())).to_string();
            }
        };

        if let Some(items) = doc.as_array() {
            if items.is_empty() {
                return error_response(
                    Value::Null,
                    &Error::invalid_request("batch must not be empty"),
                )
                .to_string();
            }
            let mut responses = Vec::new();
            for item in items {
                if let Some(response) = self.handle_one(item) {
                    responses.push(response);
                }
            }
            if responses.is_empty() {
                return String::new();
            }
            return Value::Array(responses).to_string();
        }

        match self.handle_one(&doc) {
            Some(response) => response.to_string(),
            None => String::new(),
        }
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}
