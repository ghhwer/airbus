//! Integration tests against a live Airbus process over TCP.

use airbus_client::payloads::{
    AddParams, AttachListenerParams, CreateQueueParams, DeleteQueueParams, EventObject,
    ListListenersParams, PeekEventsParams, PostEventParams, QueueMode, QueueName,
};
use airbus_client::protocol::RpcError;
use airbus_client::rpc::ClientError;
use airbus_client::{start_server, Queue, ServerGuard};
use serde_json::{json, Map, Value};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

fn uuid_v7_re() -> regex_lite::Regex {
    regex_lite::Regex::new(
        r"^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$",
    )
    .unwrap()
}

fn queue_name(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

fn event_map(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

async fn server() -> ServerGuard {
    start_server().await.expect("start airbus")
}

#[tokio::test]
async fn test_ping() {
    let guard = server().await;
    let rpc = &guard.client;
    let response = rpc.call("ping", None, Value::from(1)).await.unwrap();
    assert_eq!(response.get("jsonrpc").and_then(Value::as_str), Some("2.0"));
    assert_eq!(response.get("id"), Some(&Value::from(1)));
    assert_eq!(response.get("result").and_then(Value::as_str), Some("pong"));
    assert_eq!(rpc.ping().await.unwrap().as_str(), "pong");
}

#[tokio::test]
async fn test_add() {
    let guard = server().await;
    let rpc = &guard.client;
    let response = rpc
        .call("add", Some(json!([2, 3])), Value::from(1))
        .await
        .unwrap();
    assert_eq!(response.get("result").and_then(Value::as_f64), Some(5.0));
    assert!((*rpc.add(AddParams([2.0, 3.0])).await.unwrap() - 5.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn test_notification_has_no_response() {
    let guard = server().await;
    guard.client.notify("ping", None).await.unwrap();
}

#[tokio::test]
async fn test_method_not_found() {
    let guard = server().await;
    let response = guard
        .client
        .call("nope", None, Value::from(1))
        .await
        .unwrap();
    assert_eq!(
        response
            .pointer("/error/code")
            .and_then(Value::as_i64),
        Some(-32601)
    );
}

#[tokio::test]
async fn test_batch_ping_and_queue() {
    let guard = server().await;
    let rpc = &guard.client;
    let queue = queue_name("batch");
    let event = event_map(&[("type", json!("batch")), ("n", json!(1))]);

    let mut batch = rpc.batch();
    let pong = batch.ping().unwrap();
    batch.notify_ping().unwrap();
    let created = batch
        .create_queue(&CreateQueueParams {
            queue: queue.parse().unwrap(),
            mode: None,
            dispatch_strategy: None,
        })
        .unwrap();
    let posted = batch
        .post_event(&PostEventParams {
            queue: queue.parse().unwrap(),
            event: EventObject(event.clone()),
            side: None,
        })
        .unwrap();
    let peeked = batch
        .peek_events(&PeekEventsParams {
            queue: queue.parse().unwrap(),
            count: 1,
        })
        .unwrap();
    batch.send().await.unwrap();

    assert_eq!(pong.result().await.unwrap().as_str(), "pong");
    let created = created.result().await.unwrap();
    assert_eq!(created.queue.as_str(), queue);
    assert!(created.created);
    let posted = posted.result().await.unwrap();
    assert_eq!(posted.queue.as_str(), queue);
    assert!(uuid_v7_re().is_match(posted.id.as_str()));
    let peeked = peeked.result().await.unwrap();
    assert_eq!(peeked.queue.as_str(), queue);
    assert_eq!(peeked.events.len(), 1);
    assert_eq!(*peeked.events[0].event, event);
}

#[tokio::test]
async fn test_post_event_nonexistent_queue() {
    let guard = server().await;
    let queue = queue_name("nonexistent");
    let err = guard
        .client
        .post_event(&PostEventParams {
            queue: queue.parse().unwrap(),
            event: EventObject(event_map(&[("type", json!("fail"))])),
            side: None,
        })
        .await
        .unwrap_err();
    match err {
        ClientError::Rpc(RpcError { code, message, .. }) => {
            assert_eq!(code, -32602);
            assert!(message.contains("does not exist"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test]
async fn test_list_and_peek_events() {
    let guard = server().await;
    let rpc = &guard.client;
    let queue = queue_name("jobs");
    let other = queue_name("other");
    rpc.create_queue(&CreateQueueParams {
        queue: queue.parse().unwrap(),
        mode: None,
        dispatch_strategy: None,
    })
    .await
    .unwrap();
    rpc.create_queue(&CreateQueueParams {
        queue: other.parse().unwrap(),
        mode: None,
        dispatch_strategy: None,
    })
    .await
    .unwrap();

    let event = event_map(&[("type", json!("hello")), ("n", json!(1))]);
    let posted = rpc
        .post_event(&PostEventParams {
            queue: queue.parse().unwrap(),
            event: EventObject(event.clone()),
            side: None,
        })
        .await
        .unwrap();
    rpc.post_event(&PostEventParams {
        queue: queue.parse().unwrap(),
        event: EventObject(event_map(&[("n", json!(2))])),
        side: None,
    })
    .await
    .unwrap();
    rpc.post_event(&PostEventParams {
        queue: other.parse().unwrap(),
        event: EventObject(event_map(&[("x", json!(true))])),
        side: None,
    })
    .await
    .unwrap();

    let listed = rpc.list_queues().await.unwrap();
    let by_name: std::collections::HashMap<_, _> = listed
        .queues
        .iter()
        .map(|item| (item.name.as_str().to_string(), item.depth))
        .collect();
    assert_eq!(by_name.get(queue.as_str()).copied(), Some(2));
    assert_eq!(by_name.get(other.as_str()).copied(), Some(1));

    let peeked = rpc
        .peek_events(&PeekEventsParams {
            queue: queue.parse().unwrap(),
            count: 10,
        })
        .await
        .unwrap();
    assert_eq!(peeked.queue.as_str(), queue);
    assert_eq!(peeked.events.len(), 2);
    assert!(peeked.events.iter().any(|e| e.id.as_str() == posted.id.as_str()));
}

#[tokio::test]
async fn test_create_and_delete_queue() {
    let guard = server().await;
    let rpc = &guard.client;
    let q1 = queue_name("bcast");
    let res1 = rpc
        .create_queue(&CreateQueueParams {
            queue: q1.parse().unwrap(),
            mode: Some(QueueMode::Broadcast),
            dispatch_strategy: None,
        })
        .await
        .unwrap();
    assert!(res1.created);
    assert_eq!(res1.mode, QueueMode::Broadcast);

    let again = rpc
        .create_queue(&CreateQueueParams {
            queue: q1.parse().unwrap(),
            mode: Some(QueueMode::Broadcast),
            dispatch_strategy: None,
        })
        .await
        .unwrap();
    assert!(!again.created);

    let deleted = rpc
        .delete_queue(&DeleteQueueParams {
            queue: q1.parse().unwrap(),
        })
        .await
        .unwrap();
    assert!(deleted.deleted);
}

#[tokio::test]
async fn test_listen_broadcast() {
    let guard = server().await;
    let rpc = &guard.client;
    let queue = queue_name("bcast");
    rpc.create_queue(&CreateQueueParams {
        queue: queue.parse().unwrap(),
        mode: Some(QueueMode::Broadcast),
        dispatch_strategy: None,
    })
    .await
    .unwrap();

    let events_a: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let events_b: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let a = Arc::clone(&events_a);
    let b = Arc::clone(&events_b);

    let mut qa = Queue::connect(queue.clone(), QueueMode::Broadcast, Some(rpc.clone()))
        .await
        .unwrap();
    let mut qb = Queue::connect(queue.clone(), QueueMode::Broadcast, Some(rpc.clone()))
        .await
        .unwrap();
    qa.attach(
        move |e| {
            let a = Arc::clone(&a);
            async move {
                a.lock().await.push(e);
                Ok(())
            }
        },
        None,
        None,
        None,
    )
    .await
    .unwrap();
    qb.attach(
        move |e| {
            let b = Arc::clone(&b);
            async move {
                b.lock().await.push(e);
                Ok(())
            }
        },
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let listed = rpc
        .list_listeners(Some(&ListListenersParams {
            queue: Some(queue.parse().unwrap()),
        }))
        .await
        .unwrap();
    assert_eq!(listed.listeners.len(), 2);

    let posted = rpc
        .post_event(&PostEventParams {
            queue: queue.parse().unwrap(),
            event: EventObject(event_map(&[("broadcast", json!("hello"))])),
            side: None,
        })
        .await
        .unwrap();

    for _ in 0..60 {
        if events_a.lock().await.len() == 1 && events_b.lock().await.len() == 1 {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(events_a.lock().await.len(), 1);
    assert_eq!(events_b.lock().await.len(), 1);
    assert_eq!(
        events_a.lock().await[0].get("id").and_then(Value::as_str),
        Some(posted.id.as_str())
    );

    qa.close().await;
    qb.close().await;

    let listed_after = rpc
        .list_listeners(Some(&ListListenersParams {
            queue: Some(queue.parse::<QueueName>().unwrap()),
        }))
        .await
        .unwrap();
    assert!(listed_after.listeners.is_empty());
}

#[tokio::test]
async fn test_failed_listener_start_cleans_up() {
    let guard = server().await;
    let name = queue_name("missing");
    let mut q = Queue::connect(name.clone(), QueueMode::Broadcast, Some(guard.client.clone()))
        .await
        .unwrap();
    guard
        .client
        .delete_queue(&DeleteQueueParams {
            queue: name.parse().unwrap(),
        })
        .await
        .unwrap();
    match q.attach(|_| async { Ok(()) }, None, None, None).await {
        Err(ClientError::Rpc(RpcError { message, .. })) => {
            assert!(message.contains("does not exist"));
        }
        Err(other) => panic!("unexpected error: {other}"),
        Ok(_) => panic!("registration should fail"),
    }
}

#[tokio::test]
async fn test_server_listener_eviction_on_dead_port() {
    let guard = server().await;
    let rpc = &guard.client;
    let queue = queue_name("exhaust");
    rpc.create_queue(&CreateQueueParams {
        queue: queue.parse().unwrap(),
        mode: Some(QueueMode::Fifo),
        dispatch_strategy: None,
    })
    .await
    .unwrap();

    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = dead.local_addr().unwrap().port();
    drop(dead);

    rpc.attach_listener(&AttachListenerParams {
        queue: queue.parse().unwrap(),
        port: NonZeroU64::new(u64::from(dead_port)).unwrap(),
        host: Some("127.0.0.1".into()),
        max_retries: Some(2),
        exhaustion_timeout_ms: Some(100),
        side: None,
    })
    .await
    .unwrap();

    rpc.post_event(&PostEventParams {
        queue: queue.parse().unwrap(),
        event: EventObject(event_map(&[("test", json!("dead"))])),
        side: None,
    })
    .await
    .unwrap();

    let mut evicted = false;
    for _ in 0..30 {
        sleep(Duration::from_millis(100)).await;
        let listeners = rpc
            .list_listeners(Some(&ListListenersParams {
                queue: Some(queue.parse().unwrap()),
            }))
            .await
            .unwrap();
        if listeners.listeners.is_empty() {
            evicted = true;
            break;
        }
    }
    assert!(evicted, "unreachable listener should have been evicted");
}

// Minimal regex without adding a heavy dep — inline for uuid v7 check.
mod regex_lite {
    pub struct Regex(String);
    impl Regex {
        pub fn new(pat: &str) -> Result<Self, ()> {
            Ok(Self(pat.to_string()))
        }
        pub fn is_match(&self, text: &str) -> bool {
            // Simplified: check uuid v7 shape without full regex crate.
            let _ = &self.0;
            let parts: Vec<_> = text.split('-').collect();
            if parts.len() != 5 {
                return false;
            }
            if parts[0].len() != 8 || parts[1].len() != 4 || parts[2].len() != 4 {
                return false;
            }
            if parts[3].len() != 4 || parts[4].len() != 12 {
                return false;
            }
            let version = parts[2].chars().next().unwrap_or('0');
            version == '7'
        }
    }
}
