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

### Queue modes vs dispatch strategy

These are **two different knobs**. Do not confuse them:

| Concept | Wire field | What it controls |
| -------- | ---------- | ---------------- |
| **Queue mode** | `mode` | How events are routed (fan-out, competing consumer, or host↔device). Set once at `create_queue`. |
| **Dispatch strategy** | `dispatch_strategy` | How a **fifo** queue picks among multiple listeners. **Fifo only.** |

`create_queue` **rejects** `dispatch_strategy` when `mode` is `broadcast` or `full-duplex` (including when `mode` is omitted and defaults to `broadcast`). Error: `dispatch_strategy is only valid when mode is fifo`. Omitting the field is always fine.

### Queue Modes

Airbus supports **three** modes (`broadcast`, `fifo`, `full-duplex`):

```
Broadcast (fan-out)                      FIFO (competing consumers)
───────────────────                      ──────────────────────────

       ┌───────────────┐                        ┌───────────────┐
       │ Queue (Event) │                        │ Queue (Event) │
       └───┬───┬───┬───┘                        └───────┬───────┘
           │   │   │                                    │ round_robin / single_node
   ┌───────┘   │   └───────┐                            │
   ▼           ▼           ▼                            ▼
Listener 1  Listener 2  Listener 3                  one listener
(all receive)                                       (per event)

Full-duplex (cross-route)
─────────────────────────
  host listener  ←──events──→  device listener
  (exactly one per side; `side` required on attach/post)
```

#### 1. `broadcast`
- **Semantics**: Every attached listener receives every event.
- **Listeners**: Many allowed.
- **`dispatch_strategy`**: Must not be set.
- **Use cases**: Notifications, cache invalidation, SSE / telemetry fan-out.

#### 2. `fifo`
- **Semantics**: Each event goes to **exactly one** listener (competing consumers).
- **`dispatch_strategy`** (optional; default `round_robin`):
  - `round_robin` — cycle through healthy listeners; many may attach.
  - `single_node` — **at most one** listener may attach; a second `attach_listener` from a **different host** fails with `fifo single_node queue already has a listener`. Reattaching from the **same host** (same or new port) replaces the prior registration (crash recovery).
- **Use cases**: Job workers, exclusive single consumer pipelines.

#### 3. `full-duplex`
- **Semantics**: Exactly one `host` and one `device` listener; each event is delivered only to the **opposite** side of the publisher.
- **`side`**: Required on `attach_listener` / `post_event` (`host` or `device`); rejected on other modes.
- **`dispatch_strategy`**: Must not be set. Exclusivity is always one listener per side (not controlled by strategy). Reattaching from the same host on a side (same or new port) replaces the stale registration.
- **Use cases**: Agent ↔ server terminal buses (e.g. IFT remote clients).

### Queue readiness (`queue_ready`)

Mode-aware readiness is decided by the engine (not re-implemented in clients):

| Mode | `ready` when |
|------|----------------|
| `fifo` / `broadcast` | Queue exists |
| `full-duplex` | Queue exists and both host and device listeners are attached |

Missing queues return `{ ready: false }` (not an error). Preferred client API: `Queue.is_ready()`.

### Deleting a queue

`delete_queue` removes the named queue from the registry (buffered events and attached listeners go with it). Missing queues return `{ deleted: false }` (idempotent, not an error). Preferred client API: `Queue.delete()`.

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
    "id": "0191eb73-8a39-7f41-a6cd-2895b6c3109a",
    "event": {
      "task_name": "reindex_search",
      "priority": 1
    }
  }
}
```

The listener must process the event and return a `listener_event_result`:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "status": "ok"
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
5. If a listener process crashes and calls `attach_listener` again before exhaustion eviction, Airbus **replaces** the stale registration when:
   - the new attach uses the same `host:port` (all modes), or
   - for exclusive slots (`fifo`/`single_node`, `full-duplex` side), the new attach uses the **same host** even if the port changed.
   A different host still fails exclusivity checks as before. `broadcast` / `fifo`/`round_robin` keep allowing multiple listeners on one host at different ports.

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
