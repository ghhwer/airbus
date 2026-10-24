# Python client (`airbus-client`)

Install from PyPI and talk to a running Airbus daemon over TCP.

```bash
pip install airbus-client
```

Endpoint defaults: `AIRBUS_URL`, or `AIRBUS_HOST` / `AIRBUS_PORT`, else `127.0.0.1:9097`.

## Quick start

```python
from airbus_client import RpcClient

client = RpcClient()  # env defaults
assert client.ping() == "pong"

# Or pin an endpoint:
client = RpcClient("127.0.0.1", 9097)
client = RpcClient.from_url("10.0.0.5:9097")
```

## Preferred API: `Queue`

Most apps use the named-queue handle for create / post / attach:

```python
from airbus_client import DispatchStrategy, Queue, QueueMode

q = Queue("pipeline_jobs")
q.create(QueueMode.fifo, dispatch_strategy=DispatchStrategy.round_robin)

def on_event(event: dict) -> None:
    # event shape: {"id": "...", "queue": "...", "event": {...}}
    print(event["event"])

listener = q.attach(on_event)
assert q.post({"type": "job", "n": 1})
assert q.is_ready()

q.close()  # detach + stop local listener
# or: with Queue("pipeline_jobs") as q: ...
```

### Full-duplex (host ↔ device)

```python
from airbus_client import DuplexSide, Queue, QueueMode

host = Queue("link", side=DuplexSide.host)
device = Queue("link", side=DuplexSide.device)

host.create(QueueMode.full_duplex)
host.attach(lambda e: print("host got", e["event"]))
device.attach(lambda e: print("device got", e["event"]))

host.post({"from": "host"})    # → device
device.post({"from": "device"})  # → host
```

`try_attach` / `post` return `None` / `False` when the queue is missing (useful for device
backoff). See [Queues & Dispatch](../queues-and-dispatch.md) for mode rules.

## Raw RPC: `RpcClient`

Typed methods mirror the OpenRPC catalog. Params/results live in `airbus_client.payloads`.

```python
from airbus_client import RpcClient, QueueMode
from airbus_client.payloads import (
    CreateQueueParams,
    PeekEventsParams,
    PostEventParams,
)

client = RpcClient()
client.create_queue(CreateQueueParams(queue="jobs", mode=QueueMode.broadcast))
posted = client.post_event(PostEventParams(queue="jobs", event={"hello": True}))
print(posted.id)  # UUIDv7

peeked = client.peek_events(PeekEventsParams(queue="jobs", count=10))
for item in peeked.events:
    print(item.id, item.event)

print(client.list_queues().queues)
```

Other methods: `delete_queue`, `attach_listener`, `detach_listener`, `list_listeners`,
`queue_ready`, plus `ping` / `add` for smoke tests. Escape hatch: `call` / `notify`.

### Listen without `Queue`

```python
with client.listen("jobs", on_event=print) as listener:
    client.post_event(PostEventParams(queue="jobs", event={"n": 1}))
    # listener.port / listener.listener_id / listener.events
```

The context manager binds an ephemeral port, calls `attach_listener`, and detaches on exit.

### Batches

Calls inside `with client.batch()` are deferred into one JSON-RPC batch document. Typed
methods return a `Pending` until the block exits successfully:

```python
with client.batch():
    pong = client.ping()
    created = client.create_queue(CreateQueueParams(queue="batch-q"))
    posted = client.post_event(PostEventParams(queue="batch-q", event={"n": 1}))

assert pong.result == "pong"
assert created.result.created
assert posted.result.id
```

Nested batches are not supported.

## Errors

RPC failures raise `RpcError` (`code`, `message`, optional `data`). Invalid params and
missing queues typically use code `-32602`.

## Local daemon for tests

```python
from airbus_client import start_server

with start_server() as rpc:
    assert rpc.ping() == "pong"
```

Requires an `airbus` binary on `AIRBUS_BIN` (default `out/airbus` after `make build`).

## Package layout

| Import | Role |
| ------ | ---- |
| `airbus_client` | Public: `RpcClient`, `Queue`, `EventListener`, modes, `start_server` |
| `airbus_client.payloads` | Generated `*Params` / `*Result` |
| `protocol` / `binary` / `endpoint` | Internals |

Package README: [`client-py/README.md`](../../client-py/README.md).
