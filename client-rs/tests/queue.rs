//! Named-queue handle integration tests.

use airbus_client::payloads::{DeleteQueueParams, DispatchStrategy, QueueReadyParams};
use airbus_client::rpc::ClientError;
use airbus_client::{start_server, DuplexSide, Queue, QueueMode};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify};
use tokio::time::sleep;

fn queue_name(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

fn event_map(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

#[tokio::test]
async fn test_full_duplex_cross_route() {
    let guard = start_server().await.unwrap();
    let rpc = guard.client.clone();
    let name = queue_name("duplex");
    let host_got: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let device_got: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let ready = Arc::new(Notify::new());

    let mut host = Queue::connect(name.clone(), QueueMode::FullDuplex, Some(rpc.clone()))
        .await
        .unwrap()
        .with_side(DuplexSide::Host);
    let mut device = Queue::connect(name.clone(), QueueMode::FullDuplex, Some(rpc.clone()))
        .await
        .unwrap()
        .with_side(DuplexSide::Device);

    let hg = Arc::clone(&host_got);
    host.attach(
        move |e| {
            let hg = Arc::clone(&hg);
            async move {
                hg.lock().await.push(e);
                Ok(())
            }
        },
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let dg = Arc::clone(&device_got);
    let ready2 = Arc::clone(&ready);
    assert!(device
        .try_attach(
            move |e| {
                let dg = Arc::clone(&dg);
                let ready2 = Arc::clone(&ready2);
                async move {
                    dg.lock().await.push(e);
                    ready2.notify_one();
                    Ok(())
                }
            },
            None,
            None,
            None,
        )
        .await
        .unwrap()
        .is_some());
    assert!(host.is_ready().await.unwrap());

    assert!(host
        .post(event_map(&[("from", json!("host")), ("n", json!(1))]))
        .await
        .unwrap());
    tokio::time::timeout(Duration::from_secs(2), ready.notified())
        .await
        .expect("device event");
    assert_eq!(
        device_got.lock().await.last().unwrap()["event"]["from"],
        json!("host")
    );

    host.close().await;
    let ready3 = Arc::new(Notify::new());
    let hg = Arc::clone(&host_got);
    let r = Arc::clone(&ready3);
    host.attach(
        move |e| {
            let hg = Arc::clone(&hg);
            let r = Arc::clone(&r);
            async move {
                hg.lock().await.push(e);
                r.notify_one();
                Ok(())
            }
        },
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(device
        .post(event_map(&[("from", json!("device")), ("n", json!(2))]))
        .await
        .unwrap());
    tokio::time::timeout(Duration::from_secs(2), ready3.notified())
        .await
        .expect("host event");
    assert_eq!(
        host_got.lock().await.last().unwrap()["event"]["from"],
        json!("device")
    );

    host.close().await;
    device.close().await;
}

#[tokio::test]
async fn test_ensure_rejects_mode_mismatch() {
    let guard = start_server().await.unwrap();
    let name = queue_name("mismatch");
    Queue::connect(name.clone(), QueueMode::Fifo, Some(guard.client.clone()))
        .await
        .unwrap();
    match Queue::connect(name, QueueMode::Broadcast, Some(guard.client.clone())).await {
        Err(ClientError::Rpc(e)) => assert!(e.message.contains("exists with mode="), "{e}"),
        Err(other) => panic!("unexpected: {other}"),
        Ok(_) => panic!("expected mismatch error"),
    }
}

#[tokio::test]
async fn test_ensure_rejects_strategy_mismatch() {
    let guard = start_server().await.unwrap();
    let name = queue_name("strat");
    Queue::connect_with(
        name.clone(),
        QueueMode::Fifo,
        Some(DispatchStrategy::RoundRobin),
        Some(guard.client.clone()),
    )
    .await
    .unwrap();
    match Queue::connect_with(
        name,
        QueueMode::Fifo,
        Some(DispatchStrategy::SingleNode),
        Some(guard.client.clone()),
    )
    .await
    {
        Err(ClientError::Rpc(e)) => assert!(e.message.contains("strategy="), "{e}"),
        Err(other) => panic!("unexpected: {other}"),
        Ok(_) => panic!("expected mismatch error"),
    }
}

#[tokio::test]
async fn test_try_attach_backs_off_when_queue_deleted() {
    let guard = start_server().await.unwrap();
    let name = queue_name("missing");
    let mut device = Queue::connect(name.clone(), QueueMode::FullDuplex, Some(guard.client.clone()))
        .await
        .unwrap()
        .with_side(DuplexSide::Device);
    guard
        .client
        .delete_queue(&DeleteQueueParams {
            queue: name.parse().unwrap(),
        })
        .await
        .unwrap();
    assert!(device
        .try_attach(|_| async { Ok(()) }, None, None, None)
        .await
        .unwrap()
        .is_none());
    assert!(!device.post(event_map(&[("n", json!(1))])).await.unwrap());
    assert!(!device.is_ready().await.unwrap());
}

#[tokio::test]
async fn test_fifo_and_broadcast_ready_when_ensured() {
    let guard = start_server().await.unwrap();
    let rpc = guard.client.clone();
    let fifo_name = queue_name("fifo");
    let bcast_name = queue_name("bcast");

    let fifo = Queue::connect(fifo_name.clone(), QueueMode::Fifo, Some(rpc.clone()))
        .await
        .unwrap();
    assert!(fifo.is_ready().await.unwrap());

    let bcast = Queue::connect(bcast_name, QueueMode::Broadcast, Some(rpc.clone()))
        .await
        .unwrap();
    assert!(bcast.is_ready().await.unwrap());

    assert!(rpc
        .queue_ready(&QueueReadyParams {
            queue: fifo_name.parse().unwrap(),
        })
        .await
        .unwrap()
        .ready);
}

#[tokio::test]
async fn test_queue_delete() {
    let guard = start_server().await.unwrap();
    let name = queue_name("gone");
    let mut q = Queue::connect(name, QueueMode::Broadcast, Some(guard.client.clone()))
        .await
        .unwrap();
    assert!(q.delete().await.unwrap());
    assert!(!q.is_ready().await.unwrap());
    assert!(!q.delete().await.unwrap());
    assert!(!q.post(event_map(&[("n", json!(1))])).await.unwrap());
    sleep(Duration::from_millis(10)).await;
}
