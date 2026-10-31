# Rust client (`airbus-client`)

Async Tokio SDK for the Airbus JSON-RPC event bus.

```bash
cargo add airbus-client
```

Endpoint defaults: `AIRBUS_URL`, or `AIRBUS_HOST` / `AIRBUS_PORT`, else `127.0.0.1:9097`.

Push listeners register a callback host the daemon dials for `on_event`. The SDK
auto-discovers that address; against a **remote** bus, override with
`AIRBUS_LISTEN_HOST` or `Queue::with_listen_host` if the detected IP is not reachable
from the daemon.

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



## Attach and queues: `Queue`

`Queue::connect` / `connect_with` **require a mode** and **ensure** the queue (network
I/O): create if missing, or error if it already exists with a different mode/strategy.
Reuse a connection with `Some(rpc)`.

```rust
use airbus_client::{DispatchStrategy, Queue, QueueMode};
use serde_json::{json, Map};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut q = Queue::connect_with(
        "pipeline_jobs",
        QueueMode::Fifo,
        Some(DispatchStrategy::RoundRobin), // fifo only; None → round_robin
        None,
    )
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

```rust
let mut q = Queue::connect("pipeline_jobs", QueueMode::Fifo, None)
    .await?
    .with_listen_host("10.0.0.42"); // pin if needed
```

### Full-duplex (host ↔ device)

```rust
use airbus_client::{DuplexSide, Queue, QueueMode};
use serde_json::{json, Map};

let mut host = Queue::connect("link", QueueMode::FullDuplex, None)
    .await?
    .with_side(DuplexSide::Host);
let mut device = Queue::connect("link", QueueMode::FullDuplex, None)
    .await?
    .with_side(DuplexSide::Device);

host.attach(|e| async move { println!("host {e}"); Ok(()) }, Some(100), None, None)
    .await?;
device
    .attach(|e| async move { println!("device {e}"); Ok(()) }, Some(100), None, None)
    .await?;

let mut from_host = Map::new();
from_host.insert("from".into(), json!("host"));
host.post(from_host).await?;
```

`try_attach` returns `Ok(None)` when the queue was deleted under you. `post` returns
`Ok(false)` in the same case. Mode rules: [Queues & Dispatch](../queues-and-dispatch.md).

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

Push delivery still goes through `Queue::attach` (reuse the client with
`Queue::connect("jobs", QueueMode::Broadcast, Some(rpc.clone())).await?`). Raw
`attach_listener` only registers a host/port the daemon should dial — it does not
bind a local listener for you.

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
| root re-exports                    | `Queue`, `RpcClient`, modes, `start_server` (`EventListener` from `attach`) |
| `payloads`                         | Generated `*Params` / `*Result`                              |
| `protocol` / `endpoint` / `binary` | Internals                                                    |


Package README: `[client-rs/README.md](../../client-rs/README.md)`.