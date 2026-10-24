//! Named-queue handle integration tests.

use airbus_client::payloads::{CreateQueueParams, QueueReadyParams};
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

    let mut host = Queue::new(name.clone(), Some(rpc.clone()))
        .unwrap()
        .with_side(DuplexSide::Host);
    let mut device = Queue::new(name.clone(), Some(rpc.clone()))
        .unwrap()
        .with_side(DuplexSide::Device);

    assert!(host.create(QueueMode::FullDuplex, None).await.unwrap());
    assert!(!host.create(QueueMode::FullDuplex, None).await.unwrap());

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
async fn test_device_try_attach_backs_off_when_missing() {
    let guard = start_server().await.unwrap();
    let name = queue_name("missing");
    let mut device = Queue::new(name, Some(guard.client.clone()))
        .unwrap()
        .with_side(DuplexSide::Device);
    assert!(device
        .try_attach(|_| async { Ok(()) }, None, None, None)
        .await
        .unwrap()
        .is_none());
    assert!(!device.post(event_map(&[("n", json!(1))])).await.unwrap());
    assert!(!device.is_ready().await.unwrap());
}

#[tokio::test]
async fn test_fifo_and_broadcast_ready_when_queue_exists() {
    let guard = start_server().await.unwrap();
    let rpc = guard.client.clone();
    let fifo_name = queue_name("fifo");
    let bcast_name = queue_name("bcast");

    let fifo = Queue::new(fifo_name.clone(), Some(rpc.clone())).unwrap();
    assert!(!fifo.is_ready().await.unwrap());
    assert!(fifo.create(QueueMode::Fifo, None).await.unwrap());
    assert!(fifo.is_ready().await.unwrap());

    let bcast = Queue::new(bcast_name, Some(rpc.clone())).unwrap();
    assert!(bcast.create(QueueMode::Broadcast, None).await.unwrap());
    assert!(bcast.is_ready().await.unwrap());

    assert!(rpc
        .queue_ready(&QueueReadyParams {
            queue: fifo_name.parse().unwrap(),
        })
        .await
        .unwrap()
        .ready);
    rpc.create_queue(&CreateQueueParams {
        queue: queue_name("raw").parse().unwrap(),
        mode: Some(QueueMode::Fifo),
        dispatch_strategy: None,
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn test_queue_delete() {
    let guard = start_server().await.unwrap();
    let name = queue_name("gone");
    let mut q = Queue::new(name, Some(guard.client.clone())).unwrap();
    assert!(q.create(QueueMode::Broadcast, None).await.unwrap());
    assert!(q.delete().await.unwrap());
    assert!(!q.is_ready().await.unwrap());
    assert!(!q.delete().await.unwrap());
    assert!(!q.post(event_map(&[("n", json!(1))])).await.unwrap());
    sleep(Duration::from_millis(10)).await;
}
