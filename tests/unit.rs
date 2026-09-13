use airbus::app::{decode_params, event_object, queue_name, AppService, InvalidParams};
use airbus::bind_app_service;
use airbus::io::rpc_server::RpcServer;
use airbus::io::server::parse_listen;
use airbus::proto::payloads::{
    AddParams, AttachListenerParams, CreateQueueParams, DetachListenerParams, DispatchStrategy,
    PeekEventsParams, PostEventParams, QueueMode, QueueReadyParams,
};
use airbus::proto::rpc::{
    self, Error, Server, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR,
};
use airbus::runtime::generate_uuidv7;
use airbus::runtime::queue::QueueManager;
use serde_json::{json, Value};
use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

fn test_router() -> Server {
    let mut server = Server::new();
    server.add("ping", |_| Ok(Value::String("pong".into())));
    server.add("add", |params| {
        let arr = params
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or_else(|| Error::invalid_params("add expects [a, b]"))?;
        let a = arr[0]
            .as_f64()
            .or_else(|| arr[0].as_i64().map(|n| n as f64))
            .unwrap();
        let b = arr[1]
            .as_f64()
            .or_else(|| arr[1].as_i64().map(|n| n as f64))
            .unwrap();
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
    let out: Value =
        serde_json::from_str(&test_router().handle(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#))
            .unwrap();
    assert_eq!(out["jsonrpc"], "2.0");
    assert_eq!(out["id"], 1);
    assert_eq!(out["result"], "pong");
}

#[test]
fn rpc_add() {
    let sum: Value = serde_json::from_str(
        &test_router().handle(r#"{"jsonrpc":"2.0","method":"add","params":[2,3],"id":1}"#),
    )
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
    let out: Value =
        serde_json::from_str(&test_router().handle(r#"{"jsonrpc":"1.0","method":"ping","id":1}"#))
            .unwrap();
    assert_eq!(out["error"]["code"], INVALID_REQUEST);
}

#[test]
fn rpc_batch_ping_and_queue() {
    let out: Value = serde_json::from_str(&app_rpc_server().on_bytes(
        br#"[{"jsonrpc":"2.0","method":"ping","id":1},{"jsonrpc":"2.0","method":"create_queue","params":{"queue":"batch"},"id":2},{"jsonrpc":"2.0","method":"post_event","params":{"queue":"batch","event":{"n":1}},"id":3},{"jsonrpc":"2.0","method":"peek_events","params":{"queue":"batch"},"id":4}]"#,
    ))
    .unwrap();
    assert!(out.is_array());
    assert_eq!(out.as_array().unwrap().len(), 4);
    assert_eq!(out[0]["id"], 1);
    assert_eq!(out[0]["result"], "pong");
    assert_eq!(out[1]["id"], 2);
    assert_eq!(out[1]["result"]["queue"], "batch");
    assert_eq!(out[1]["result"]["created"], true);
    assert_eq!(out[2]["id"], 3);
    assert_eq!(out[2]["result"]["queue"], "batch");
    assert!(looks_like_uuidv7(out[2]["result"]["id"].as_str().unwrap()));
    assert_eq!(out[3]["id"], 4);
    assert_eq!(out[3]["result"]["queue"], "batch");
    assert_eq!(out[3]["result"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(out[3]["result"]["events"][0]["event"], json!({"n": 1}));
    assert!(looks_like_uuidv7(
        out[3]["result"]["events"][0]["id"].as_str().unwrap()
    ));
}

#[test]
fn rpc_get_events_method_not_found() {
    let out: Value = serde_json::from_str(&app_rpc_server().on_bytes(
        br#"{"jsonrpc":"2.0","method":"get_events","params":{"queue":"jobs"},"id":1}"#,
    ))
    .unwrap();
    assert_eq!(out["error"]["code"], METHOD_NOT_FOUND);
    assert_eq!(out["id"], 1);
}

#[test]
fn rpc_request_builder() {
    let req = rpc::request("ping", json!(1));
    assert_eq!(req["jsonrpc"], "2.0");
    assert_eq!(req["method"], "ping");
    assert_eq!(req["id"], 1);
}

fn create_params(
    queue: &str,
    mode: Option<QueueMode>,
    dispatch_strategy: Option<DispatchStrategy>,
) -> CreateQueueParams {
    let mut value = json!({ "queue": queue });
    if let Some(m) = mode {
        value["mode"] = json!(m);
    }
    if let Some(s) = dispatch_strategy {
        value["dispatch_strategy"] = json!(s);
    }
    decode_params(&value).unwrap()
}

fn post_params(queue: &str, event: Value) -> PostEventParams {
    PostEventParams {
        queue: queue_name(queue).unwrap(),
        event: event_object(event).unwrap(),
        side: None,
    }
}

fn peek_params(queue: &str, count: Option<i64>) -> PeekEventsParams {
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
    id.len() == 36
        && id.as_bytes()[8] == b'-'
        && id.as_bytes()[13] == b'-'
        && id.as_bytes()[14] == b'7'
        && id.as_bytes()[18] == b'-'
        && id.as_bytes()[23] == b'-'
}

#[test]
fn service_post_event() {
    let app = AppService::new();
    app.create_queue(create_params("jobs", None, None)).unwrap();
    let posted = app
        .post_event(post_params("jobs", json!({"type": "hello"})))
        .unwrap();
    assert_eq!(posted.queue.as_str(), "jobs");
    assert!(looks_like_uuidv7(posted.id.as_str()));
}

#[test]
fn service_post_event_nonexistent_queue() {
    let app = AppService::new();
    let err = app
        .post_event(post_params("nonexistent", json!({"type": "hello"})))
        .unwrap_err();
    assert!(err.message.contains("does not exist"));
}

#[test]
fn service_post_event_invalid_params() {
    assert!(decode_params::<PostEventParams>(&Value::Null).is_err());
    assert!(decode_params::<PostEventParams>(&json!({})).is_err());
    assert!(decode_params::<PostEventParams>(&json!({"queue": "jobs"})).is_err());
}

#[test]
fn service_list_queues_default() {
    let app = AppService::new();
    let listed = app.list_queues();
    assert_eq!(listed.queues.len(), 1);
    assert_eq!(listed.queues[0].name.as_str(), "demo");
    assert_eq!(listed.queues[0].mode, QueueMode::Broadcast);
    assert_eq!(listed.queues[0].listener_count, 0);
}

#[test]
fn service_list_and_peek_events() {
    let app = AppService::new();
    app.create_queue(create_params("jobs", None, None)).unwrap();
    app.create_queue(create_params("other", None, None))
        .unwrap();
    let posted = app
        .post_event(post_params("jobs", json!({"n": 1})))
        .unwrap();
    app.post_event(post_params("jobs", json!({"n": 2})))
        .unwrap();
    app.post_event(post_params("other", json!({"x": true})))
        .unwrap();

    let listed = app.list_queues();
    assert_eq!(listed.queues.len(), 3);
    assert_eq!(listed.queues[0].name.as_str(), "demo");
    assert_eq!(listed.queues[1].name.as_str(), "jobs");
    assert_eq!(listed.queues[1].depth, 2);
    assert_eq!(listed.queues[2].name.as_str(), "other");
    assert_eq!(listed.queues[2].depth, 1);

    let peeked = app.peek_events(peek_params("jobs", Some(10))).unwrap();
    assert_eq!(peeked.events.len(), 2);
    let ids: Vec<&str> = peeked.events.iter().map(|e| e.id.as_str()).collect();
    assert!(ids.contains(&posted.id.as_str()));
    let ns: std::collections::BTreeSet<i64> = peeked
        .events
        .iter()
        .filter_map(|e| e.event.0.get("n").and_then(|v| v.as_i64()))
        .collect();
    assert_eq!(ns, [1, 2].into());

    // peek is non-destructive
    assert_eq!(app.list_queues().queues[1].depth, 2);
    assert_eq!(
        app.peek_events(peek_params("jobs", None))
            .unwrap()
            .events
            .len(),
        1
    );
}

#[test]
fn service_peek_events_missing_queue_is_empty() {
    let app = AppService::new();
    let peeked = app.peek_events(peek_params("missing", None)).unwrap();
    assert!(peeked.events.is_empty());
}

#[test]
fn service_peek_events_invalid_params() {
    assert!(decode_params::<PeekEventsParams>(&Value::Null).is_err());
    assert!(decode_params::<PeekEventsParams>(&json!({})).is_err());
}

#[test]
fn service_queues_are_isolated() {
    let app = AppService::new();
    app.create_queue(create_params("a", None, None)).unwrap();
    app.create_queue(create_params("b", None, None)).unwrap();
    app.post_event(post_params("a", json!({"from": "a"})))
        .unwrap();
    app.post_event(post_params("b", json!({"from": "b"})))
        .unwrap();
    let from_a = app.peek_events(peek_params("a", None)).unwrap();
    assert_eq!(from_a.events.len(), 1);
    assert_eq!(from_a.events[0].event.0.get("from"), Some(&json!("a")));
    let from_b = app.peek_events(peek_params("b", None)).unwrap();
    assert_eq!(from_b.events.len(), 1);
    assert_eq!(from_b.events[0].event.0.get("from"), Some(&json!("b")));
}

#[test]
fn queue_manager_isolates_queues() {
    let manager = QueueManager::new();
    manager
        .create_queue("a", QueueMode::Broadcast, DispatchStrategy::RoundRobin)
        .unwrap();
    manager
        .create_queue("b", QueueMode::Broadcast, DispatchStrategy::RoundRobin)
        .unwrap();
    manager
        .publish("a", generate_uuidv7(), json!(1), None)
        .unwrap();
    manager
        .publish("b", generate_uuidv7(), json!(2), None)
        .unwrap();
    let from_a = manager.peek("a", 8);
    assert_eq!(from_a.len(), 1);
    assert_eq!(from_a[0].1, 1);
    let from_b = manager.peek("b", 8);
    assert_eq!(from_b.len(), 1);
    assert_eq!(from_b[0].1, 2);
}

#[test]
fn queue_manager_list_and_peek() {
    let manager = QueueManager::new();
    manager
        .create_queue("jobs", QueueMode::Broadcast, DispatchStrategy::RoundRobin)
        .unwrap();
    manager
        .create_queue("other", QueueMode::Broadcast, DispatchStrategy::RoundRobin)
        .unwrap();
    let id = generate_uuidv7();
    manager
        .publish("jobs", id, json!({"n": 1}), None)
        .unwrap();
    manager
        .publish("jobs", generate_uuidv7(), json!({"n": 2}), None)
        .unwrap();
    manager
        .publish("other", generate_uuidv7(), json!(true), None)
        .unwrap();

    let listed = manager.list();
    assert_eq!(
        listed,
        vec![
            ("demo".into(), 0, QueueMode::Broadcast, 0),
            ("jobs".into(), 2, QueueMode::Broadcast, 0),
            ("other".into(), 1, QueueMode::Broadcast, 0)
        ]
    );

    let peeked = manager.peek("jobs", 10);
    assert_eq!(peeked.len(), 2);
    assert!(peeked
        .iter()
        .any(|(eid, value)| *eid == id && *value == json!({"n": 1})));
    assert_eq!(
        manager.list(),
        vec![
            ("demo".into(), 0, QueueMode::Broadcast, 0),
            ("jobs".into(), 2, QueueMode::Broadcast, 0),
            ("other".into(), 1, QueueMode::Broadcast, 0)
        ]
    );
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
    let out: Value =
        serde_json::from_str(&server.on_bytes(br#"{"jsonrpc":"2.0","method":"ping","id":1}"#))
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

#[test]
fn http_rpc_ping_and_static() {
    use airbus::io::http_server::{handle_request_for_test, safe_join};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("airbus-http-ui-{stamp}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("index.html"), b"<html>ok</html>").unwrap();
    let resources = fs::canonicalize(&dir).unwrap();

    let mut server = RpcServer::new();
    bind_app_service(&mut server, Arc::new(AppService::new()));

    let rpc = handle_request_for_test(
        "POST",
        "/rpc",
        br#"{"jsonrpc":"2.0","method":"ping","id":1}"#,
        &resources,
        |bytes| server.on_bytes(bytes),
    );
    let rpc_text = String::from_utf8_lossy(&rpc);
    assert!(rpc_text.contains("HTTP/1.1 200"));
    assert!(rpc_text.contains("\"result\":\"pong\""));

    let page = handle_request_for_test("GET", "/", b"", &resources, |_| String::new());
    let page_text = String::from_utf8_lossy(&page);
    assert!(page_text.contains("HTTP/1.1 200"));
    assert!(page_text.contains("<html>ok</html>"));

    assert!(safe_join(&resources, "../etc/passwd").is_none());
    let _ = fs::remove_dir_all(&dir);
}

struct MockListener {
    port: u16,
    receiver: mpsc::Receiver<Value>,
    stop_handle: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for MockListener {
    fn drop(&mut self) {
        self.stop_handle
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

fn spawn_mock_listener() -> MockListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r_clone = running.clone();

    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        while r_clone.load(std::sync::atomic::Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    use std::io::{Read, Write};
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    loop {
                        match stream.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&chunk[..n]);
                                if let Ok(val) = serde_json::from_slice::<Value>(&buf) {
                                    let _ = tx.send(val.clone());
                                    let id = val.get("id").cloned().unwrap_or(json!(1));
                                    let ack = json!({"jsonrpc": "2.0", "result": {"status": "ok"}, "id": id});
                                    let ack_bytes = serde_json::to_vec(&ack).unwrap();
                                    let _ = stream.write_all(&ack_bytes);
                                    let _ = stream.flush();
                                    break;
                                }
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(std::time::Duration::from_millis(10));
                            }
                            Err(_) => break,
                        }
                    }
                    drop(stream);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    MockListener {
        port,
        receiver: rx,
        stop_handle: running,
    }
}

#[test]
fn test_create_queue_lifecycle() {
    let app = AppService::new();
    let res = app
        .create_queue(create_params(
            "tasks",
            Some(QueueMode::Fifo),
            Some(DispatchStrategy::RoundRobin),
        ))
        .unwrap();
    assert_eq!(res.queue.as_str(), "tasks");
    assert_eq!(res.mode, QueueMode::Fifo);
    assert!(res.created);

    // Idempotent re-creation returns created: false
    let res2 = app
        .create_queue(create_params(
            "tasks",
            Some(QueueMode::Fifo),
            Some(DispatchStrategy::RoundRobin),
        ))
        .unwrap();
    assert_eq!(res2.queue.as_str(), "tasks");
    assert!(!res2.created);

    let existing = app
        .create_queue(create_params("tasks", None, None))
        .unwrap();
    assert!(!existing.created);
    assert_eq!(existing.mode, QueueMode::Fifo);
    assert_eq!(
        app.list_queues()
            .queues
            .iter()
            .find(|q| q.name.as_str() == "tasks")
            .unwrap()
            .mode,
        existing.mode
    );
}

#[test]
fn rpc_rejects_unicode_listener_id_without_panicking() {
    let request = json!({
        "jsonrpc": "2.0", "method": "detach_listener", "id": 1,
        "params": {"listener_id": format!("a€{}", "0".repeat(28))}
    });
    let response: Value =
        serde_json::from_str(&app_rpc_server().on_bytes(&serde_json::to_vec(&request).unwrap()))
            .unwrap();
    assert_eq!(response["error"]["code"], INVALID_PARAMS);
}

#[test]
fn rpc_rejects_out_of_range_listener_ports() {
    let server = app_rpc_server();
    for port in [0, 65536, 65537, u64::MAX] {
        let request = json!({
            "jsonrpc": "2.0", "method": "attach_listener", "id": 1,
            "params": {"queue": "demo", "port": port}
        });
        let response: Value =
            serde_json::from_str(&server.on_bytes(&serde_json::to_vec(&request).unwrap())).unwrap();
        assert_eq!(response["error"]["code"], INVALID_PARAMS, "port {port}");
    }
}

#[test]
fn broadcast_detach_releases_pending_delivery_without_adding_late_listeners() {
    use airbus::runtime::queue::{ListenerRegistration, Queue};
    use std::time::Duration;

    let mut queue = Queue::new(QueueMode::Broadcast, DispatchStrategy::RoundRobin);
    let first = generate_uuidv7();
    let second = generate_uuidv7();
    let late = generate_uuidv7();
    for id in [first, second] {
        queue
            .attach_listener(ListenerRegistration::new(
                id,
                "127.0.0.1".into(),
                12345,
                3,
                Duration::from_secs(10),
                None,
            ))
            .unwrap();
    }
    let event_id = generate_uuidv7();
    queue.publish(event_id, json!({"n": 1}), None).unwrap();
    assert_eq!(queue.broadcast_listeners(event_id).len(), 2);
    queue.acknowledge_broadcast(&event_id, &first);
    assert_eq!(queue.depth(), 1);
    queue
        .attach_listener(ListenerRegistration::new(
            late,
            "127.0.0.1".into(),
            12345,
            3,
            Duration::from_secs(10),
            None,
        ))
        .unwrap();
    let pending = queue.broadcast_listeners(event_id);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, second);
    assert!(queue.detach_listener(&second));
    assert_eq!(queue.depth(), 0);
}

#[test]
fn broadcast_detach_without_acknowledgment_retains_event_for_future_listeners() {
    use airbus::runtime::queue::{ListenerRegistration, Queue};
    use std::time::Duration;

    let mut queue = Queue::new(QueueMode::Broadcast, DispatchStrategy::RoundRobin);
    let dead = generate_uuidv7();
    queue
        .attach_listener(ListenerRegistration::new(
            dead,
            "127.0.0.1".into(),
            12345,
            3,
            Duration::from_secs(10),
            None,
        ))
        .unwrap();
    let event_id = generate_uuidv7();
    queue.publish(event_id, json!({"n": 1}), None).unwrap();
    assert_eq!(queue.broadcast_listeners(event_id).len(), 1);

    // Dead listener is evicted/detached without acknowledging
    assert!(queue.detach_listener(&dead));

    // Event must NOT be dropped
    assert_eq!(queue.depth(), 1);
    assert_eq!(queue.first_event().unwrap().0, event_id);

    // When a new listener attaches, the event is delivered to it
    let healthy = generate_uuidv7();
    queue
        .attach_listener(ListenerRegistration::new(
            healthy,
            "127.0.0.1".into(),
            12346,
            3,
            Duration::from_secs(10),
            None,
        ))
        .unwrap();
    let recipients = queue.broadcast_listeners(event_id);
    assert_eq!(recipients.len(), 1);
    assert_eq!(recipients[0].id, healthy);

    // When healthy acknowledges, event is completed
    queue.acknowledge_broadcast(&event_id, &healthy);
    assert_eq!(queue.depth(), 0);
}

#[test]
fn test_attach_and_detach_listener() {
    let app = AppService::new();
    app.create_queue(create_params("stream", None, None))
        .unwrap();

    let attach: AttachListenerParams = decode_params(&json!({
        "queue": "stream",
        "port": 12345,
        "host": "127.0.0.1"
    }))
    .unwrap();

    let res = app.attach_listener(attach).unwrap();
    assert_eq!(res.queue.as_str(), "stream");
    assert_eq!(res.status.as_str(), "attached");
    let lid = res.listener_id.clone();

    let list = app
        .list_listeners(Some(decode_params(&json!({"queue": "stream"})).unwrap()))
        .unwrap();
    assert_eq!(list.listeners.len(), 1);
    assert_eq!(list.listeners[0].id.as_str(), lid.as_str());
    assert_eq!(list.listeners[0].port.get(), 12345);

    let detach: DetachListenerParams = decode_params(&json!({
        "listener_id": lid
    }))
    .unwrap();
    let dres = app.detach_listener(detach).unwrap();
    assert!(dres.detached);

    let list_after = app
        .list_listeners(Some(decode_params(&json!({"queue": "stream"})).unwrap()))
        .unwrap();
    assert!(list_after.listeners.is_empty());
}

#[test]
fn test_broadcast_delivery() {
    let app = AppService::new();
    app.create_queue(create_params("bcast", Some(QueueMode::Broadcast), None))
        .unwrap();

    let mock1 = spawn_mock_listener();
    let mock2 = spawn_mock_listener();

    let a1: AttachListenerParams = decode_params(&json!({
        "queue": "bcast",
        "port": mock1.port,
        "host": "127.0.0.1"
    }))
    .unwrap();
    let a2: AttachListenerParams = decode_params(&json!({
        "queue": "bcast",
        "port": mock2.port,
        "host": "127.0.0.1"
    }))
    .unwrap();

    app.attach_listener(a1).unwrap();
    app.attach_listener(a2).unwrap();

    let posted = app
        .post_event(post_params("bcast", json!({"msg": "hello broadcast"})))
        .unwrap();

    // Both listeners should receive the event
    let ev1 = mock1
        .receiver
        .recv_timeout(std::time::Duration::from_secs(3))
        .expect("mock1 received event");
    let ev2 = mock2
        .receiver
        .recv_timeout(std::time::Duration::from_secs(3))
        .expect("mock2 received event");

    assert_eq!(ev1["method"], "on_event");
    assert_eq!(ev1["params"]["queue"], "bcast");
    assert_eq!(ev1["params"]["id"], posted.id.as_str());
    assert_eq!(ev1["params"]["event"]["msg"], "hello broadcast");

    assert_eq!(ev2["method"], "on_event");
    assert_eq!(ev2["params"]["queue"], "bcast");
    assert_eq!(ev2["params"]["id"], posted.id.as_str());
    assert_eq!(ev2["params"]["event"]["msg"], "hello broadcast");

    // Queue should clean the event once acknowledged
    thread::sleep(std::time::Duration::from_millis(150));
    let peek = app.peek_events(peek_params("bcast", None)).unwrap();
    assert!(peek.events.is_empty());
}

#[test]
fn test_fifo_round_robin_delivery() {
    let app = AppService::new();
    app.create_queue(create_params(
        "workers",
        Some(QueueMode::Fifo),
        Some(DispatchStrategy::RoundRobin),
    ))
    .unwrap();

    let mock1 = spawn_mock_listener();
    let mock2 = spawn_mock_listener();

    let a1: AttachListenerParams = decode_params(&json!({
        "queue": "workers",
        "port": mock1.port,
        "host": "127.0.0.1"
    }))
    .unwrap();
    let a2: AttachListenerParams = decode_params(&json!({
        "queue": "workers",
        "port": mock2.port,
        "host": "127.0.0.1"
    }))
    .unwrap();

    app.attach_listener(a1).unwrap();
    app.attach_listener(a2).unwrap();

    for i in 0..4 {
        app.post_event(post_params("workers", json!({"task": i})))
            .unwrap();
    }

    let mut mock1_events = Vec::new();
    let mut mock2_events = Vec::new();

    for _ in 0..2 {
        mock1_events.push(
            mock1
                .receiver
                .recv_timeout(std::time::Duration::from_secs(3))
                .expect("mock1 received task"),
        );
        mock2_events.push(
            mock2
                .receiver
                .recv_timeout(std::time::Duration::from_secs(3))
                .expect("mock2 received task"),
        );
    }

    assert_eq!(mock1_events.len(), 2);
    assert_eq!(mock2_events.len(), 2);

    // Queue should be empty after consumption
    thread::sleep(std::time::Duration::from_millis(150));
    let peek = app.peek_events(peek_params("workers", None)).unwrap();
    assert!(peek.events.is_empty());
}

#[test]
fn test_client_unreachable_exhaustion() {
    let app = AppService::new();
    app.create_queue(create_params("exhaust", Some(QueueMode::Fifo), None))
        .unwrap();

    // Find an unused port and immediately close it
    let dead_port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };

    let attach: AttachListenerParams = decode_params(&json!({
        "queue": "exhaust",
        "port": dead_port,
        "host": "127.0.0.1",
        "max_retries": 2,
        "exhaustion_timeout_ms": 100
    }))
    .unwrap();

    app.attach_listener(attach).unwrap();

    // Verify it is initially registered
    let list = app
        .list_listeners(Some(decode_params(&json!({"queue": "exhaust"})).unwrap()))
        .unwrap();
    assert_eq!(list.listeners.len(), 1);

    // Post an event to trigger delivery attempts to the dead port
    app.post_event(post_params("exhaust", json!({"ping": true})))
        .unwrap();

    // Wait for the dispatcher to retry and evict the dead listener
    let start = std::time::Instant::now();
    let mut evicted = false;
    while start.elapsed() < std::time::Duration::from_secs(4) {
        thread::sleep(std::time::Duration::from_millis(100));
        let list = app
            .list_listeners(Some(decode_params(&json!({"queue": "exhaust"})).unwrap()))
            .unwrap();
        if list.listeners.is_empty() {
            evicted = true;
            break;
        }
    }

    assert!(evicted, "Dead listener should be evicted upon exhaustion");
}

#[test]
fn full_duplex_requires_side_and_cross_routes() {
    use airbus::proto::payloads::DuplexSide;
    use airbus::runtime::queue::{ListenerRegistration, Queue};
    use std::time::Duration;

    let mut queue = Queue::new(QueueMode::FullDuplex, DispatchStrategy::RoundRobin);
    assert!(queue
        .attach_listener(ListenerRegistration::new(
            generate_uuidv7(),
            "127.0.0.1".into(),
            1,
            3,
            Duration::from_secs(10),
            None,
        ))
        .is_err());

    let host_id = generate_uuidv7();
    queue
        .attach_listener(ListenerRegistration::new(
            host_id,
            "127.0.0.1".into(),
            1,
            3,
            Duration::from_secs(10),
            Some(DuplexSide::Host),
        ))
        .unwrap();
    assert!(queue
        .attach_listener(ListenerRegistration::new(
            generate_uuidv7(),
            "127.0.0.1".into(),
            2,
            3,
            Duration::from_secs(10),
            Some(DuplexSide::Host),
        ))
        .is_err());

    queue
        .attach_listener(ListenerRegistration::new(
            generate_uuidv7(),
            "127.0.0.1".into(),
            3,
            3,
            Duration::from_secs(10),
            Some(DuplexSide::Device),
        ))
        .unwrap();

    let event_id = generate_uuidv7();
    queue
        .publish(event_id, json!({"ping": true}), Some(DuplexSide::Host))
        .unwrap();
    let ready = queue.first_duplex_event_ready().unwrap();
    assert_eq!(ready.0, event_id);
    assert_eq!(ready.2, DuplexSide::Device);
}

#[test]
fn full_duplex_app_service_rejects_invalid_side_usage() {
    let app = AppService::new();
    app.create_queue(create_params("chan", Some(QueueMode::FullDuplex), None))
        .unwrap();
    // round_robin is allowed on full-duplex (both sides can still attach).
    app.create_queue(create_params(
        "chan2",
        Some(QueueMode::FullDuplex),
        Some(DispatchStrategy::RoundRobin),
    ))
    .unwrap();

    let bad_attach: AttachListenerParams = decode_params(&json!({
        "queue": "chan",
        "port": 12345,
    }))
    .unwrap();
    assert!(app.attach_listener(bad_attach).is_err());

    let bad_post = post_params("chan", json!({"n": 1}));
    assert!(app.post_event(bad_post).is_err());

    app.create_queue(create_params("plain", Some(QueueMode::Broadcast), None))
        .unwrap();
    let sided: AttachListenerParams = decode_params(&json!({
        "queue": "plain",
        "port": 12345,
        "side": "host",
    }))
    .unwrap();
    assert!(app.attach_listener(sided).is_err());
}

#[test]
fn fifo_single_node_rejects_second_listener() {
    use airbus::runtime::queue::{ListenerRegistration, Queue};
    use std::time::Duration;

    let mut queue = Queue::new(QueueMode::Fifo, DispatchStrategy::SingleNode);
    queue
        .attach_listener(ListenerRegistration::new(
            generate_uuidv7(),
            "127.0.0.1".into(),
            1,
            3,
            Duration::from_secs(10),
            None,
        ))
        .unwrap();
    assert!(queue
        .attach_listener(ListenerRegistration::new(
            generate_uuidv7(),
            "127.0.0.1".into(),
            2,
            3,
            Duration::from_secs(10),
            None,
        ))
        .is_err());
}

#[test]
fn full_duplex_buffers_until_peer_attaches() {
    use airbus::proto::payloads::DuplexSide;
    use airbus::runtime::queue::{ListenerRegistration, Queue};
    use std::time::Duration;

    let mut queue = Queue::new(QueueMode::FullDuplex, DispatchStrategy::RoundRobin);
    queue
        .attach_listener(ListenerRegistration::new(
            generate_uuidv7(),
            "127.0.0.1".into(),
            1,
            3,
            Duration::from_secs(10),
            Some(DuplexSide::Host),
        ))
        .unwrap();
    let event_id = generate_uuidv7();
    queue
        .publish(event_id, json!({"hello": true}), Some(DuplexSide::Host))
        .unwrap();
    assert!(queue.first_duplex_event_ready().is_none());
    assert_eq!(queue.depth(), 1);

    queue
        .attach_listener(ListenerRegistration::new(
            generate_uuidv7(),
            "127.0.0.1".into(),
            2,
            3,
            Duration::from_secs(10),
            Some(DuplexSide::Device),
        ))
        .unwrap();
    assert_eq!(queue.first_duplex_event_ready().unwrap().0, event_id);
}

#[test]
fn queue_ready_missing_fifo_broadcast_and_duplex() {
    let app = AppService::new();
    let missing: QueueReadyParams = decode_params(&json!({ "queue": "nope" })).unwrap();
    assert!(!app.queue_ready(missing).ready);

    app.create_queue(create_params("fifo-q", Some(QueueMode::Fifo), None))
        .unwrap();
    let fifo: QueueReadyParams = decode_params(&json!({ "queue": "fifo-q" })).unwrap();
    assert!(app.queue_ready(fifo).ready);

    app.create_queue(create_params("bcast-q", Some(QueueMode::Broadcast), None))
        .unwrap();
    let bcast: QueueReadyParams = decode_params(&json!({ "queue": "bcast-q" })).unwrap();
    assert!(app.queue_ready(bcast).ready);

    app.create_queue(create_params("duplex-q", Some(QueueMode::FullDuplex), None))
        .unwrap();
    let duplex_name = "duplex-q";
    let not_ready: QueueReadyParams = decode_params(&json!({ "queue": duplex_name })).unwrap();
    assert!(!app.queue_ready(not_ready).ready);

    let host_port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let device_port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    app.attach_listener(decode_params(&json!({
        "queue": duplex_name,
        "port": host_port,
        "host": "127.0.0.1",
        "side": "host"
    }))
    .unwrap())
    .unwrap();
    let one_side: QueueReadyParams = decode_params(&json!({ "queue": duplex_name })).unwrap();
    assert!(!app.queue_ready(one_side).ready);

    app.attach_listener(decode_params(&json!({
        "queue": duplex_name,
        "port": device_port,
        "host": "127.0.0.1",
        "side": "device"
    }))
    .unwrap())
    .unwrap();
    let both: QueueReadyParams = decode_params(&json!({ "queue": duplex_name })).unwrap();
    let result = app.queue_ready(both);
    assert!(result.ready);
    assert_eq!(result.queue.as_str(), duplex_name);
}

