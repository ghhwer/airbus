# Airbus Queues & Event Dispatch

This document explains the queuing semantics, dispatch algorithms, listener lifecycle, and
delivery reliability mechanisms in Airbus.

## Queue Lifecycle & Configuration

Queues in Airbus are named in-memory channels configured with specific delivery semantics.

### Creating a Queue

Prefer the thin `Queue` handle (mode is a create argument). Raw `RpcClient` + params remain available for advanced use:

```python
from airbus_client import Queue, QueueMode, DispatchStrategy

q = Queue("pipeline_jobs")  # AIRBUS_URL / host+port env, else 127.0.0.1:9097
q.create(QueueMode.fifo, dispatch_strategy=DispatchStrategy.round_robin)
```

Equivalent raw RPC:

```python
from airbus_client import RpcClient
from airbus_client.payloads import CreateQueueParams, QueueMode, DispatchStrategy

client = RpcClient()  # same env defaults
client.create_queue(
    CreateQueueParams(
        queue="pipeline_jobs",
        mode=QueueMode.fifo,
        dispatch_strategy=DispatchStrategy.round_robin,
    )
)
```

### Queue Modes

Airbus supports three modes configured per queue:

```
Broadcast Mode (Fan-Out)                 FIFO Mode (Competing Consumers)
────────────────────────                 ──────────────────────────────────

       ┌───────────────┐                        ┌───────────────┐
       │ Queue (Event) │                        │ Queue (Event) │
       └───┬───┬───┬───┘                        └───────┬───────┘
           │   │   │                                    │ (Round Robin / single_node)
   ┌───────┘   │   └───────┐                            │
   ▼           ▼           ▼                            ▼
Listener 1  Listener 2  Listener 3                  Listener 1
(receives)  (receives)  (receives)                  (one consumer)
```

#### 1. `broadcast` Mode
- **Semantics**: Every attached listener receives every event published to the queue.
- **Use Cases**: System notifications, cache invalidation, SSE live-sync broadcasts, telemetry fan-out.
- **Delivery**: An event delivery record is created for each attached listener. An event is considered fully delivered when all active listeners have acknowledged it.

#### 2. `fifo` Mode
- **Semantics**: Competing consumers model. Each published event is routed to exactly one available listener.
- **Use Cases**: Background processing jobs, heavy task execution, worker agent dispatch.
- **Dispatch Strategies**:
  - `round_robin` (default): Cycles sequentially through healthy, available listeners.
  - `single_node`: Prefer a single exclusive consumer when configured for exclusivity.

#### 3. `full-duplex` Mode
- **Semantics**: Exactly one `host` and one `device` listener; events cross-route to the opposite side.
- **Use Cases**: Agent ↔ server terminal buses (IFT remote clients).
- **Side**: Required on `attach_listener` / `post_event` (`host` or `device`).

### Queue readiness (`queue_ready`)

Mode-aware readiness is decided by the engine (not re-implemented in clients):

| Mode | `ready` when |
|------|----------------|
| `fifo` / `broadcast` | Queue exists |
| `full-duplex` | Queue exists and both host and device listeners are attached |

Missing queues return `{ ready: false }` (not an error). Preferred client API: `Queue.is_ready()`.

---

## Event Ingestion & UUIDv7 Identification

When a client calls `post_event`:

1. **Validation**: The queue existence and event JSON structure are validated against `post_event_params.schema.json`.
2. **UUIDv7 Generation**: Airbus generates a monotonically increasing, time-sortable **UUIDv7** (RFC 9562) identifier:
   - 48-bit UNIX millisecond timestamp ensures chronological ordering.
   - 74 bits of pseudo-random data guarantee collision resistance.
   - Formatted as standard 36-character hyphenated UUID (e.g. `0191eb73-8a39-7f41-a6cd-2895b6c3109a`).
3. **Queue Storage**: The event is placed into the queue's internal buffer.
4. **Immediate Ack**: The publisher receives a `PostEventResult` containing `{ id, queue }`. The publisher is unblocked immediately; event dispatch occurs asynchronously in the background.

---

## Listener Architecture & Callbacks

Airbus uses an **inverted callback architecture** for event delivery: instead of clients polling continuously, listeners open a TCP port and register with Airbus. Airbus then initiates RPC calls (`on_event`) to the listener.

### 1. Attaching a Listener

A consumer process starts a TCP server, then calls `attach_listener`:

```python
from airbus_client.payloads import AttachListenerParams

res = client.attach_listener(
    AttachListenerParams(
        queue="pipeline_jobs",
        host="127.0.0.1",
        port=19050,
        max_retries=5,
        exhaustion_timeout_ms=15000,
    )
)
listener_id = res.listener_id.root
```

### 2. The `on_event` Callback

When an event is ready for dispatch, an Airbus dispatcher worker connects to the listener's TCP socket and sends a JSON-RPC request for `on_event`:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "on_event",
  "params": {
    "queue": "pipeline_jobs",
    "event_id": "0191eb73-8a39-7f41-a6cd-2895b6c3109a",
    "event": {
      "task_name": "reindex_search",
      "priority": 1
    },
    "attempt": 1
  }
}
```

The listener must process the event and return a `listener_event_result`:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "status": "acknowledged"
  }
}
```

If the listener fails or cannot accept the task, it may reject it:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "status": "rejected",
    "error": "worker busy"
  }
}
```

### 3. Detaching a Listener

When a worker shuts down, it detaches to avoid receiving further callbacks:

```python
from airbus_client.payloads import DetachListenerParams

client.detach_listener(DetachListenerParams(listener_id=listener_id))
```

---

## Retries, Deadlines & Fault Tolerance

Airbus manages delivery failures automatically based on per-listener configuration:

| Parameter | Default | Meaning |
| --------- | ------- | ------- |
| `max_retries` | `3` | Maximum number of delivery attempts before exhausting delivery |
| `exhaustion_timeout_ms` | `10000` (10s) | Maximum total time window allowed for event acknowledgment |

### Failure Handling Flow

1. If the listener's TCP port is unreachable, or the socket times out, the attempt fails.
2. If the listener responds with `status: "rejected"`, the attempt fails.
3. Upon failure:
   - In `broadcast` mode: The dispatcher records the failure and schedules a retry for that specific listener if `attempt < max_retries`.
   - In `fifo` mode: The event is immediately requeued and dispatched to another available listener.
4. If an event exceeds its `exhaustion_timeout_ms` or `max_retries`, it is marked exhausted.

---

## Non-Destructive Event Peeking

Clients and monitoring tools can inspect queued events without consuming them using `peek_events`:

```python
from airbus_client.payloads import PeekEventsParams

peek = client.peek_events(PeekEventsParams(queue="pipeline_jobs", count=5))
for item in peek.events:
    print(f"Event {item.id.root}: {item.event.root}")
```

This is used extensively by the [Airbus Debug UI](overview.md#interactive-debug-ui) to show live queue state without disrupting active consumers.

---

## Python Integration Example

Below is a complete pattern for implementing a worker service with `airbus-client`:

```python
import socket
import threading
from airbus_client import RpcClient
from airbus_client.payloads import AttachListenerParams, DetachListenerParams

def run_worker():
    # 1. Connect to Airbus daemon
    client = RpcClient("127.0.0.1", 9097)

    # 2. Attach listener
    attach_res = client.attach_listener(
        AttachListenerParams(
            queue="tasks",
            host="127.0.0.1",
            port=19099,
        )
    )
    listener_id = attach_res.listener_id.root

    try:
        # 3. Listen on port 19099 for on_event callbacks
        # (handle JSON-RPC on_event invocations from Airbus)
        pass
    finally:
        # 4. Clean teardown
        client.detach_listener(DetachListenerParams(listener_id=listener_id))
```
