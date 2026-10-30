# Rust client (`airbus-client`)

Async Tokio SDK for the Airbus JSON-RPC event bus.

```bash
cargo add airbus-client
```

Endpoint defaults: `AIRBUS_URL`, or `AIRBUS_HOST` / `AIRBUS_PORT`, else `127.0.0.1:9097`.

Push listeners register a callback host the daemon dials for `on_event`. By default the
SDK auto-discovers the local egress IP toward the Airbus peer (or keeps `127.0.0.1`
when the peer is loopback). Override with `AIRBUS_LISTEN_HOST` or
`Queue::with_listen_host` / `EventListener::host` — required when auto-detection cannot
see a routable path to a **remote** bus.

## Quick start

```rust
use airbus_client::RpcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = RpcClient::from_env()?;
    assert_eq!(client.ping().await?.as_str(), "pong");

    // Or pin an endpoint:
    let client = RpcClient::new("127.0.0.1", 9097).with_timeout(std::time::Duration::from_secs(2));
    let client = RpcClient::from_url("10.0.0.5:9097")?;
    Ok(())
}
```



## Preferred API: `Queue`

```rust
use airbus_client::{DispatchStrategy, Queue, QueueMode};
use serde_json::{json, Map};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut q = Queue::new("pipeline_jobs", None)?;
    q.create(QueueMode::Fifo, Some(DispatchStrategy::RoundRobin))
        .await?;

    q.attach(
        |event| async move {
            println!("{event}");
            Ok(())
        },
        Some(100), // max_events
        None,      // exhaustion_timeout_ms
        None,      // max_retries
    )
    .await?;

    let mut event = Map::new();
    event.insert("type".into(), json!("job"));
    event.insert("n".into(), json!(1));
    assert!(q.post(event).await?);
    assert!(q.is_ready().await?);

    q.close().await;
    Ok(())
}
```

Against a remote Airbus, auto-discovery usually registers a LAN/routable IP. Pin it when
needed:

```rust
let mut q = Queue::new("pipeline_jobs", None)?.with_listen_host("10.0.0.42");
// or: export AIRBUS_LISTEN_HOST=10.0.0.42
```

Local `127.0.0.1:9097` setups keep working without changes.

### Full-duplex (host ↔ device)

```rust
use airbus_client::{DuplexSide, Queue, QueueMode};
use serde_json::{json, Map};

let mut host = Queue::new("link", None)?.with_side(DuplexSide::Host);
let mut device = Queue::new("link", None)?.with_side(DuplexSide::Device);

host.create(QueueMode::FullDuplex, None).await?;
host.attach(|e| async move { println!("host {e}"); Ok(()) }, Some(100), None, None)
    .await?;
device
    .attach(|e| async move { println!("device {e}"); Ok(()) }, Some(100), None, None)
    .await?;

let mut from_host = Map::new();
from_host.insert("from".into(), json!("host"));
host.post(from_host).await?;
```

`try_attach` returns `Ok(None)` when the queue does not exist. `post` returns `Ok(false)`
in the same case. Mode rules: [Queues & Dispatch](../queues-and-dispatch.md).

## Raw RPC: `RpcClient`

Typed methods take generated params from `airbus_client::payloads`:

```rust
use airbus_client::payloads::{
    CreateQueueParams, EventObject, PeekEventsParams, PostEventParams, QueueMode,
};
use airbus_client::RpcClient;
use serde_json::{json, Map};

let rpc = RpcClient::from_env()?;
let queue = "jobs".parse()?;

rpc.create_queue(&CreateQueueParams {
    queue: queue.clone(),
    mode: Some(QueueMode::Broadcast),
    dispatch_strategy: None,
})
.await?;

let mut body = Map::new();
body.insert("hello".into(), json!(true));
let posted = rpc
    .post_event(&PostEventParams {
        queue: queue.clone(),
        event: EventObject(body),
        side: None,
    })
    .await?;
println!("{}", posted.id);

let peeked = rpc
    .peek_events(&PeekEventsParams {
        queue,
        count: 10,
    })
    .await?;
```

Also available: `delete_queue`, `attach_listener`, `detach_listener`, `list_listeners`,
`queue_ready`, `list_queues`, `ping`, `add`, plus `call` / `notify` / `raw`.

### Listen without `Queue`

```rust
let mut listener = rpc
    .listen("jobs")
    // .host("10.0.0.42")  // optional; default auto-discovers toward the peer
    .max_events(100)
    .start(|event| async move {
        println!("{event}");
        Ok(()) // ack; Err(String) → JSON-RPC error to the daemon
    })
    .await?;

listener.close().await;
```

Builder options: `host` (auto when omitted), `exhaustion_timeout_ms`, `max_retries`, `side`.

### Batches

```rust
let mut batch = rpc.batch();
let pong = batch.ping()?;
let created = batch.create_queue(&CreateQueueParams {
    queue: "batch-q".parse()?,
    mode: None,
    dispatch_strategy: None,
})?;
batch.send().await?;

assert_eq!(pong.result().await?.as_str(), "pong");
assert!(created.result().await?.created);
```

Enqueue returns `Pending<T>`; read results after `send()`.

## Errors

Failures surface as `ClientError` (`Rpc`, `Protocol`, `Io`, `Other`). RPC errors carry
`RpcError { code, message, .. }` — missing queues / bad params usually `-32602`.

## Local daemon for tests

```rust
use airbus_client::start_server;

let guard = start_server().await?;
assert_eq!(guard.client.ping().await?.as_str(), "pong");
// drops → terminates the child process
```

Requires an `airbus` binary (`AIRBUS_BIN`, default `out/airbus`).

## Crate layout


| Module                             | Role                                                         |
| ---------------------------------- | ------------------------------------------------------------ |
| root re-exports                    | `RpcClient`, `Queue`, `EventListener`, modes, `start_server` |
| `payloads`                         | Generated `*Params` / `*Result`                              |
| `protocol` / `endpoint` / `binary` | Internals                                                    |


Package README: `[client-rs/README.md](../../client-rs/README.md)`.