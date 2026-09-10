use airbus::app::{decode_params, event_object, queue_name, AppService, InvalidParams};
use airbus::bind_app_service;
use airbus::io::rpc_server::RpcServer;
use airbus::io::server::parse_listen;
use airbus::proto::payloads::{AddParams, GetEventsParams, PostEventParams};
use airbus::proto::rpc::{self, Error, Server, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR};
use airbus::runtime::generate_uuidv7;
use airbus::runtime::queue::QueueManager;
use serde_json::{json, Value};
use std::sync::Arc;

fn test_router() -> Server {
    let mut server = Server::new();
    server.add("ping", |_| Ok(Value::String("pong".into())));
    server.add("add", |params| {
        let arr = params
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or_else(|| Error::invalid_params("add expects [a, b]"))?;
        let a = arr[0].as_f64().or_else(|| arr[0].as_i64().map(|n| n as f64)).unwrap();
        let b = arr[1].as_f64().or_else(|| arr[1].as_i64().map(|n| n as f64)).unwrap();
        Ok(json!(a + b))
    });
    server
}

fn app_rpc_server() -> RpcServer {
    let mut server = RpcServer::new();
    bind_app_service(&mut server, Arc::new(AppService::new()));
    server
}

#[test]
fn rpc_ping() {
    let out: Value = serde_json::from_str(
        &test_router().handle(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#),
    )
    .unwrap();
    assert_eq!(out["jsonrpc"], "2.0");
    assert_eq!(out["id"], 1);
    assert_eq!(out["result"], "pong");
}

#[test]
fn rpc_add() {
    let sum: Value =
        serde_json::from_str(&test_router().handle(r#"{"jsonrpc":"2.0","method":"add","params":[2,3],"id":1}"#))
            .unwrap();
    assert_eq!(sum["result"].as_f64().unwrap(), 5.0);
}

#[test]
fn rpc_notification_has_no_response() {
    assert!(test_router()
        .handle(r#"{"jsonrpc":"2.0","method":"ping"}"#)
        .is_empty());
}

#[test]
fn rpc_method_not_found() {
    let out: Value =
        serde_json::from_str(&test_router().handle(r#"{"jsonrpc":"2.0","method":"nope","id":1}"#))
            .unwrap();
    assert_eq!(out["error"]["code"], METHOD_NOT_FOUND);
    assert_eq!(out["id"], 1);
}

#[test]
fn rpc_invalid_params() {
    let out: Value = serde_json::from_str(
        &test_router().handle(r#"{"jsonrpc":"2.0","method":"add","params":[1],"id":1}"#),
    )
    .unwrap();
    assert_eq!(out["error"]["code"], INVALID_PARAMS);
}

#[test]
fn rpc_parse_error() {
    let out: Value = serde_json::from_str(&test_router().handle("{")).unwrap();
    assert_eq!(out["error"]["code"], PARSE_ERROR);
    assert!(out["id"].is_null());
}

#[test]
fn rpc_invalid_request() {
    let out: Value = serde_json::from_str(
        &test_router().handle(r#"{"jsonrpc":"1.0","method":"ping","id":1}"#),
    )
    .unwrap();
    assert_eq!(out["error"]["code"], INVALID_REQUEST);
}

#[test]
fn rpc_batch_ping_and_queue() {
    let out: Value = serde_json::from_str(&app_rpc_server().on_bytes(
        br#"[{"jsonrpc":"2.0","method":"ping","id":1},{"jsonrpc":"2.0","method":"ping"},{"jsonrpc":"2.0","method":"post_event","params":{"queue":"batch","event":{"n":1}},"id":2},{"jsonrpc":"2.0","method":"get_events","params":{"queue":"batch"},"id":3}]"#,
    ))
    .unwrap();
    assert!(out.is_array());
    assert_eq!(out.as_array().unwrap().len(), 3);
    assert_eq!(out[0]["id"], 1);
    assert_eq!(out[0]["result"], "pong");
    assert_eq!(out[1]["id"], 2);
    assert_eq!(out[1]["result"]["queue"], "batch");
    assert!(looks_like_uuidv7(out[1]["result"]["id"].as_str().unwrap()));
    assert_eq!(out[2]["id"], 3);
    assert_eq!(out[2]["result"]["queue"], "batch");
    assert_eq!(out[2]["result"]["events"], json!([{"n": 1}]));
}

#[test]
fn rpc_request_builder() {
    let req = rpc::request("ping", json!(1));
    assert_eq!(req["jsonrpc"], "2.0");
    assert_eq!(req["method"], "ping");
    assert_eq!(req["id"], 1);
}

fn post_params(queue: &str, event: Value) -> PostEventParams {
    PostEventParams {
        queue: queue_name(queue).unwrap(),
        event: event_object(event).unwrap(),
    }
}

fn get_params(queue: &str, count: Option<i64>) -> GetEventsParams {
    let mut value = json!({ "queue": queue });
    if let Some(c) = count {
        value["count"] = json!(c);
    }
    decode_params(&value).unwrap()
}

#[test]
fn service_ping() {
    let app = AppService::new();
    assert_eq!(app.ping().0, "pong");
}

#[test]
fn service_add() {
    let app = AppService::new();
    let typed: AddParams = decode_params(&json!([2, 3])).unwrap();
    assert_eq!(app.add(typed).0, 5.0);
}

#[test]
fn service_add_invalid_params() {
    assert!(matches!(
        decode_params::<AddParams>(&json!([1])),
        Err(InvalidParams { .. })
    ));
    assert!(matches!(
        decode_params::<AddParams>(&json!({})),
        Err(InvalidParams { .. })
    ));
}

fn looks_like_uuidv7(id: &str) -> bool {
    id.len() == 36 && id.as_bytes()[8] == b'-' && id.as_bytes()[13] == b'-' && id.as_bytes()[14] == b'7'
        && id.as_bytes()[18] == b'-' && id.as_bytes()[23] == b'-'
}

#[test]
fn service_post_event() {
    let app = AppService::new();
    let posted = app
        .post_event(post_params("jobs", json!({"type": "hello"})))
        .unwrap();
    assert_eq!(posted.queue.as_str(), "jobs");
    assert!(looks_like_uuidv7(posted.id.as_str()));
}

#[test]
fn service_post_event_invalid_params() {
    assert!(decode_params::<PostEventParams>(&Value::Null).is_err());
    assert!(decode_params::<PostEventParams>(&json!({})).is_err());
    assert!(decode_params::<PostEventParams>(&json!({"queue": "jobs"})).is_err());
}

#[test]
fn service_get_events_after_post() {
    let app = AppService::new();
    app.post_event(post_params("jobs", json!({"n": 1})))
        .unwrap();
    let got = app.get_events(get_params("jobs", None));
    assert_eq!(got.queue.as_str(), "jobs");
    assert_eq!(got.events.len(), 1);
    assert_eq!(got.events[0].0.get("n"), Some(&json!(1)));
}

#[test]
fn service_get_events_default_count_is_one() {
    let app = AppService::new();
    app.post_event(post_params("jobs", json!({"n": 1})))
        .unwrap();
    app.post_event(post_params("jobs", json!({"n": 2})))
        .unwrap();
    assert_eq!(app.get_events(get_params("jobs", None)).events.len(), 1);
    assert_eq!(app.get_events(get_params("jobs", None)).events.len(), 1);
    assert!(app.get_events(get_params("jobs", None)).events.is_empty());
}

#[test]
fn service_get_events_respects_count() {
    let app = AppService::new();
    for n in 1..=3 {
        app.post_event(post_params("jobs", json!({"n": n})))
            .unwrap();
    }
    assert_eq!(app.get_events(get_params("jobs", Some(2))).events.len(), 2);
    assert_eq!(app.get_events(get_params("jobs", Some(8))).events.len(), 1);
}

#[test]
fn service_get_events_missing_queue_is_empty() {
    let app = AppService::new();
    let got = app.get_events(get_params("missing", None));
    assert!(got.events.is_empty());
}

#[test]
fn service_get_events_invalid_params() {
    assert!(decode_params::<GetEventsParams>(&Value::Null).is_err());
    assert!(decode_params::<GetEventsParams>(&json!({})).is_err());
}

#[test]
fn service_queues_are_isolated() {
    let app = AppService::new();
    app.post_event(post_params("a", json!({"from": "a"})))
        .unwrap();
    app.post_event(post_params("b", json!({"from": "b"})))
        .unwrap();
    let from_a = app.get_events(get_params("a", None));
    assert_eq!(from_a.events.len(), 1);
    assert_eq!(from_a.events[0].0.get("from"), Some(&json!("a")));
    let from_b = app.get_events(get_params("b", None));
    assert_eq!(from_b.events.len(), 1);
    assert_eq!(from_b.events[0].0.get("from"), Some(&json!("b")));
}

#[test]
fn queue_manager_publish_consume() {
    let manager = QueueManager::new();
    let id = generate_uuidv7();
    manager.publish("jobs", id, json!("hello")).unwrap();
    let events = manager.consume("jobs", 1);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], "hello");
}

#[test]
fn queue_manager_isolates_queues() {
    let manager = QueueManager::new();
    manager
        .publish("a", generate_uuidv7(), json!(1))
        .unwrap();
    manager
        .publish("b", generate_uuidv7(), json!(2))
        .unwrap();
    let from_a = manager.consume("a", 8);
    assert_eq!(from_a.len(), 1);
    assert_eq!(from_a[0], 1);
    let from_b = manager.consume("b", 8);
    assert_eq!(from_b.len(), 1);
    assert_eq!(from_b[0], 2);
}

#[test]
fn queue_manager_missing_queue_is_empty() {
    let manager = QueueManager::new();
    assert!(manager.consume("missing", 1).is_empty());
}

#[test]
fn queue_manager_consume_removes_payload() {
    let manager = QueueManager::new();
    manager
        .publish("jobs", generate_uuidv7(), json!("x"))
        .unwrap();
    assert_eq!(manager.consume("jobs", 1).len(), 1);
    assert!(manager.consume("jobs", 1).is_empty());
}

#[test]
fn io_parse_listen_host_port() {
    let (host, port) = parse_listen("0.0.0.0:8080").unwrap();
    assert_eq!(host, "0.0.0.0");
    assert_eq!(port, 8080);
}

#[test]
fn io_parse_listen_bare_port() {
    let (host, port) = parse_listen("9097").unwrap();
    assert_eq!(host, "127.0.0.1");
    assert_eq!(port, 9097);
}

#[test]
fn io_parse_listen_empty_host() {
    let (host, port) = parse_listen(":0").unwrap();
    assert_eq!(host, "127.0.0.1");
    assert_eq!(port, 0);
}

#[test]
fn io_rpc_server_on_bytes() {
    let mut server = RpcServer::new();
    server.route("ping", |_| Ok(Value::String("pong".into())));
    let out: Value = serde_json::from_str(
        &server.on_bytes(br#"{"jsonrpc":"2.0","method":"ping","id":1}"#),
    )
    .unwrap();
    assert_eq!(out["result"], "pong");
}

#[test]
fn io_rpc_server_notification_empty() {
    let mut server = RpcServer::new();
    server.route("ping", |_| Ok(Value::String("pong".into())));
    assert!(server
        .on_bytes(br#"{"jsonrpc":"2.0","method":"ping"}"#)
        .is_empty());
}
