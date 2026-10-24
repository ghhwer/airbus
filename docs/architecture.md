# Airbus Architecture & Runtime

This document maps layer boundaries and how work moves through the daemon.
Module and type detail lives in `src/` (and contracts in `schema/`) — open the
tree when you need implementation.

## Design Philosophy

Airbus is structured around **composition over inheritance** and **strict boundary enforcement**:

1. **Protocol purity**: The application layer has zero awareness of TCP sockets, byte framing, or JSON-RPC envelopes. It consumes strongly-typed structs and returns strongly-typed results.
2. **Envelope isolation**: Envelopes (`jsonrpc`, `id`, `method`, `params`, `result`, `error`) are parsed and constructed only in the protocol layer. Application and runtime code never build or inspect wire envelopes.
3. **Contract Single Source of Truth**: Data models originate in `schema/` and generate client/daemon types. Layers must not hand-duplicate those shapes.
4. **Decoupled dispatch**: Queues decouple publishers from consumers. Publishers get an immediate ack with a UUIDv7 event id; dispatch workers deliver asynchronously to registered listeners.

## Layer Overview

```
airbus/
├── schema/             # Contract SOT (JSON Schema + OpenRPC)
├── scripts/            # Codegen & protocol boundary enforcement
├── resources/ui/       # Static debug UI (same JSON-RPC over HTTP)
├── src/
│   ├── app/            # Application methods → runtime
│   ├── io/             # TCP / HTTP transport adapters
│   ├── proto/          # Envelope handling + generated payloads
│   ├── runtime/        # Queues, dispatch, delivery, UUIDv7
│   ├── wiring.rs       # Bind app handlers onto the RPC server
│   ├── lib.rs
│   └── main.rs         # CLI, bootstrap, signals
└── client-*/           # Language SDKs (Python, Rust, embedded, …)
```


| Layer                  | Path                           | Responsibility                                                                                            |
| ---------------------- | ------------------------------ | --------------------------------------------------------------------------------------------------------- |
| **Schema**             | `schema/`                      | Payload JSON Schema and OpenRPC method catalog.                                                           |
| **I/O & Transport**    | `src/io/`                      | TCP accept loop, optional HTTP (`POST /rpc` + static UI), RPC adapter that turns bytes into method calls. |
| **Protocol**           | `src/proto/`                   | JSON-RPC 2.0 envelopes and generated payload types.                                                       |
| **Application**        | `src/app/`                     | Typed handlers: validate, call runtime, return generated results.                                         |
| **Runtime**            | `src/runtime/`                 | Named queues, mode-aware dispatch, delivery/retries, listener dial-out (`on_event`).                      |
| **Wiring / bootstrap** | `src/wiring.rs`, `src/main.rs` | Register methods, parse CLI, spawn listeners.                                                             |
| **Clients**            | `client-*/`                    | SDKs that speak the same contract; not part of the daemon process.                                        |


**Where a change belongs**

- New/changed RPC shapes → `schema/`, then codegen ([Protocol](protocol.md)).
- Delivery semantics (modes, retries, exclusivity) → `src/runtime/` and [Queues & Dispatch](queues-and-dispatch.md).
- Transport or framing → `src/io/` / `src/proto/` (envelopes stay out of `app/` and `runtime/`).
- Method orchestration only → `src/app/` + wiring.

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
           ▼ (per connection)                          ▼ (per request)
┌─────────────────────┐                     ┌─────────────────────┐
│  Client TCP Worker  │                     │  HTTP Request Worker│
└──────────┬──────────┘                     └──────────┬──────────┘
           │                                           │
           └─────────────────────┬─────────────────────┘
                                 ▼
                     ┌───────────────────────┐
                     │  Shared queue runtime │
                     └───────────┬───────────┘
                                 │
           ┌─────────────────────┴─────────────────────┐
           ▼                                           ▼
┌─────────────────────┐                     ┌─────────────────────┐
│ Dispatcher workers  │                     │ Dispatcher workers  │
│ (per active queue)  │                     │ (dial listeners)    │
└─────────────────────┘                     └─────────────────────┘
```

1. **Accept / HTTP**: Inbound RPC is handled on connection- or request-scoped workers; they share the same application + runtime state.
2. **Shared state**: Queue registry and listener pools are synchronized; publishers are not blocked on listener I/O.
3. **Dispatch**: After `post_event` acks, background workers select listeners by queue mode and dial `on_event` (see [Queues & Dispatch](queues-and-dispatch.md)).

## Debug UI

Optional HTTP serves static assets from `resources/ui/` and the same JSON-RPC surface at `POST /rpc`. The browser speaks native JSON-RPC — no REST translation layer.

## Related

- [Protocol](protocol.md) — envelopes, framing, schema/codegen boundaries
- [Queues & Dispatch](queues-and-dispatch.md) — modes, delivery, retries, listeners
- [CONTRIBUTING.md](../CONTRIBUTING.md) — develop and change this tree
- Implementation: `src/`; method catalog: `schema/openrpc.json` + `schema/payloads/`

