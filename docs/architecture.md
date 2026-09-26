# Airbus Architecture & Runtime

This document details the internal architecture, layer composition, runtime engine, and
concurrency model of the Airbus event bus.

## Design Philosophy

Airbus is structured around **composition over inheritance** and **strict boundary enforcement**:

1. **Protocol purity**: The application layer (`AppService`) has zero awareness of TCP sockets, byte framing, or JSON-RPC envelope envelopes. It consumes strongly-typed Rust structs and returns strongly-typed results.
2. **Envelope isolation**: Envelopes (`jsonrpc`, `id`, `method`, `params`, `result`, `error`) are parsed and constructed exclusively in `src/proto/rpc.rs`. Application code never inspects or constructs JSON-RPC wire envelopes.
3. **Contract Single Source of Truth**: All data models originate in `schema/` and generate Rust and Python code. Neither layer creates manual data duplicates.
4. **Decoupled dispatch**: Queues decouple publishers from consumers. Publishers receive an immediate acknowledgment with a UUIDv7 event identifier; dispatch workers deliver events asynchronously to registered listeners.

## Layer Overview

```
airbus/
├── schema/             # Contract SOT (JSON Schema + OpenRPC)
├── scripts/            # Codegen & protocol boundary enforcement
├── resources/ui/       # Static web debug UI & browser RPC adapter
├── src/
│   ├── app/            # AppService: application methods (ping, post_event, etc.)
│   ├── io/             # TCP socket server, HTTP server, RpcServer adapter, logging
│   ├── proto/          # JSON-RPC envelope parsing + generated payload types
│   ├── runtime/        # QueueManager, Queue, Dispatcher, Delivery, UUIDv7
│   ├── wiring.rs       # Composition root: wires AppService onto RpcServer
│   ├── lib.rs          # Library exports
│   └── main.rs         # CLI parsing, server bootstrap, signal handling
└── client/             # Python airbus-client package (uv workspace member)
```

| Layer | Module Path | Responsibility |
| ----- | ----------- | -------------- |
| **Schema** | `schema/` | JSON Schema payload definitions (`payloads/*.schema.json`) and OpenRPC method catalog (`openrpc.json`). |
| **I/O & Transport** | `src/io/` | TCP listener (`server.rs`), socket utilities (`net.rs`), HTTP server (`http_server.rs`), RPC adapter (`rpc_server.rs`), and stderr logger (`log.rs`). |
| **Protocol** | `src/proto/` | JSON-RPC 2.0 wire document handling (`rpc.rs`) and generated payload types (`payloads.rs`). |
| **Runtime Engine** | `src/runtime/` | Core queuing engine: `QueueManager`, `Queue`, `Dispatcher`, delivery tracking, worker strategies, and time-ordered UUIDv7 generation. |
| **Application** | `src/app/` | `AppService`: business logic dispatching RPC methods to the runtime engine and formatting generated response types. |
| **Wiring** | `src/wiring.rs` | Binds method names (`ping`, `create_queue`, `post_event`, etc.) on `RpcServer` to `AppService` handlers. |
| **Client** | `client/` | Python SDK (`airbus_client`) with `RpcClient`, listener server, and protocol serialization. |

---

## Detailed Layer Breakdown

### 1. I/O & Networking (`src/io/`)

- `server.rs`: Implements `serve_tcp`, which binds a `TcpListener` on the configured address and accepts client connections. Each connection is handed off to a processing loop that reads newline-delimited JSON-RPC requests, delegates execution to `RpcServer`, and writes newline-delimited responses.
- `rpc_server.rs`: The transport adapter. It maintains a registry of registered method handlers. When given raw request bytes, it invokes `src/proto/rpc.rs` to parse the JSON-RPC document, dispatches to the registered handler, and serializes the result back to JSON-RPC wire bytes.
- `http_server.rs`: An optional lightweight HTTP server built on `tiny_http`. It serves:
  1. Static files from `--resources DIR` (e.g. `resources/ui/index.html`, `styles.css`, `app.js`, `protocol.js`).
  2. A `POST /rpc` endpoint that accepts standard JSON-RPC 2.0 POST requests, processes them through the shared `RpcServer`, and returns the JSON-RPC response.
- `net.rs`: Helpers for parsing socket addresses and TCP stream configuration.
- `log.rs`: Thread-safe stderr logging formatting timestamps and log levels without external dependencies.

### 2. Protocol & Wire Types (`src/proto/`)

- `rpc.rs`: Implements standard JSON-RPC 2.0 framing. Defines `RpcRequest`, `RpcResponse`, `RpcError`, and error codes (`PARSE_ERROR`, `INVALID_REQUEST`, `METHOD_NOT_FOUND`, `INVALID_PARAMS`, `INTERNAL_ERROR`). It enforces that responses contain either `result` or `error`, never both.
- `payloads.rs`: Typify-generated Rust structs corresponding to all schemas in `schema/payloads/`. Includes parameter and result types like `CreateQueueParams`, `PostEventParams`, `PeekEventsResult`, and `AttachListenerParams`.

### 3. Application Service (`src/app/`)

- `service.rs`: Defines `AppService`. It holds an `Arc<QueueManager>` and provides typed handler methods:
  - `ping(&self) -> PingResult`
  - `add(&self, params: AddParams) -> AddResult`
  - `create_queue(&self, params: CreateQueueParams) -> Result<CreateQueueResult, InvalidParams>`
  - `post_event(&self, params: PostEventParams) -> Result<PostEventResult, InvalidParams>`
  - `peek_events(&self, params: PeekEventsParams) -> Result<PeekEventsResult, InvalidParams>`
  - `list_queues(&self) -> ListQueuesResult`
  - `attach_listener(&self, params: AttachListenerParams) -> Result<AttachListenerResult, InvalidParams>`
  - `detach_listener(&self, params: DetachListenerParams) -> Result<DetachListenerResult, InvalidParams>`
  - `list_listeners(&self, params: Option<ListListenersParams>) -> Result<ListListenersResult, InvalidParams>`

Each method validates parameters, delegates state manipulation to `QueueManager`, and wraps results in generated schema types.

### 4. Runtime Queuing Engine (`src/runtime/`)

- `manager.rs`: `QueueManager` coordinates all named queues and listener registrations. Thread-safe and shared via `Arc<RwLock<...>>` / `Arc<Mutex<...>>`.
- `queue.rs`: `Queue` maintains the in-memory ring/buffer of events, tracking depth, sequence numbers, and unconsumed event state.
- `queue/delivery.rs`: Tracks inflight event deliveries, acknowledgment states, retry counters, and expiration deadlines.
- `queue/dispatcher.rs`: Runs background event dispatch loops that route newly published events to available listeners according to queue configuration.
- `queue/dispatch/`: Submodules implementing queue delivery strategies:
  - `broadcast.rs`: Broadcast delivery ensuring every attached listener receives each event.
  - `worker.rs`: Worker delivery distributing events among listeners using `round_robin` or `random` strategies.
- `queue/listener.rs`: Manages listener network state, TCP connection pooling, and callback RPC execution (`on_event`).
- `uuidv7.rs`: Custom implementation of UUIDv7 (RFC 9562) providing 128-bit time-ordered identifiers with millisecond timestamp precision and cryptographic randomness.

### 5. Composition Root (`src/wiring.rs` & `src/main.rs`)

`src/wiring.rs` provides `bind_app_service`:

```rust
pub fn bind_app_service(rpc: &mut RpcServer, app: Arc<AppService>) {
    rpc.register("ping", ...);
    rpc.register("add", ...);
    rpc.register("create_queue", ...);
    rpc.register("post_event", ...);
    rpc.register("peek_events", ...);
    rpc.register("list_queues", ...);
    rpc.register("attach_listener", ...);
    rpc.register("detach_listener", ...);
    rpc.register("list_listeners", ...);
}
```

`main.rs` initializes logging, parses CLI flags (`--listen`, `--http`, `--resources`), instantiates `AppService`, binds it to `RpcServer`, and spawns the TCP listener and optional HTTP server threads.

---

## Concurrency & Threading Model

```
                    ┌─────────────────────────┐
                    │       main thread       │
                    │   (CLI & supervisor)    │
                    └────────────┬────────────┘
                                 │
           ┌─────────────────────┴─────────────────────┐
           ▼                                           ▼
┌─────────────────────┐                     ┌─────────────────────┐
│   TCP Accept Loop   │                     │  HTTP Server Loop   │
│     (port 9097)     │                     │     (port 9098)     │
└──────────┬──────────┘                     └──────────┬──────────┘
           │                                           │
           ▼ (spawns per connection)                   ▼ (spawns per request)
┌─────────────────────┐                     ┌─────────────────────┐
│  Client TCP Worker  │                     │  HTTP Request Worker│
└──────────┬──────────┘                     └──────────┬──────────┘
           │                                           │
           └─────────────────────┬─────────────────────┘
                                 ▼
                     ┌───────────────────────┐
                     │  Shared QueueManager  │
                     │  (Arc<RwLock/Mutex>)  │
                     └───────────┬───────────┘
                                 │
           ┌─────────────────────┴─────────────────────┐
           ▼                                           ▼
┌─────────────────────┐                     ┌─────────────────────┐
│ Dispatcher Worker 1 │                     │ Dispatcher Worker N │
│ (queue: "jobs")     │                     │ (queue: "events")   │
└─────────────────────┘                     └─────────────────────┘
```

1. **Accept Loop**: The TCP listener loop accepts inbound client connections and handles them with asynchronous or per-connection workers.
2. **Shared State**: All queue data structures and listener pools are protected by synchronizing primitives (`RwLock` for queue routing, `Mutex` for delivery rings).
3. **Dispatch Loops**: When events are published to a queue, active dispatch workers match available listeners and initiate TCP callback connections (`on_event`) without blocking the publisher.

---

## Static Debug UI Integration

Airbus embeds a static web dashboard under `resources/ui/`:
- `index.html`: Clean, zero-dependency HTML dashboard.
- `styles.css`: Modern styling with dark/light theme support.
- `protocol.js`: Client-side JSON-RPC 2.0 encoder/decoder. Formats valid envelopes and parses responses.
- `app.js`: Connects to `POST /rpc`, periodically polls queue lists (`list_queues`) and listeners (`list_listeners`), provides forms to create queues, post events, and inspect events via `peek_events`.

Because the HTTP server exposes the exact same JSON-RPC methods at `POST /rpc`, the browser speaks native JSON-RPC without requiring an intermediate REST translation layer.
