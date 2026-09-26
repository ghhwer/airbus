# Airbus Overview

Airbus is a high-performance, lightweight event bus and message queue daemon implemented in
Rust, communicating over JSON-RPC 2.0 with strict cross-language protocol schemas.

## Components

- **Airbus daemon** (`src/`): Rust JSON-RPC TCP server providing event queues and dispatch.
- **Airbus Python client** (`client-py/` → `airbus_client`): publishable SDK for other projects.
- **Airbus C++ client** (`client-cpp/`): CMake library; C++ and PlatformIO release zips are packed from this tree.
- **Airbus Debug UI** (`resources/ui/`): Browser interface for queue inspection and RPC execution.
- **Container image** (`Dockerfile`): runs the daemon + UI for deployment.

```
┌─────────────────────────────────────────────────────────────────┐
│                         Your application                        │
│                                                                 │
│   airbus-client (pip / C++ / PIO) ── JSON-RPC 2.0 (TCP :9097) ──▶ Airbus   │
│                                                         daemon  │
│                              ▲                                  │
│                              │ HTTP :9098  Debug UI / POST /rpc │
└─────────────────────────────────────────────────────────────────┘
```

## Key Capabilities

1. **Named Event Queues**: Create and manage queues dynamically with configured delivery modes.
2. **Dual Queue Modes**:
   - `Broadcast`: Every attached listener receives every published event.
   - `Worker`: Events are load-balanced across listeners (`round_robin`, `random`).
3. **Time-Ordered Event Identifiers**: Monotonically increasing, time-sortable UUIDv7 IDs.
4. **Push-Based Listener Dispatch**: `on_event` RPC callbacks with acknowledgments.
5. **Configurable Retries & Timeouts**: `max_retries`, `exhaustion_timeout_ms`.
6. **Non-Destructive Peeking**: `peek_events` without advancing consumers.
7. **Strict Contract Boundaries**: Schema-first JSON Schema + OpenRPC; generated Rust/Python types.
8. **Interactive Debug UI**: HTTP static server + JSON-RPC at `POST /rpc`.

## Quick Start

```bash
make setup && make build && make run
# TCP 0.0.0.0:9097 — HTTP http://127.0.0.1:9098/
```

Or with Docker:

```bash
docker compose up --build
```

```text
airbus --listen [host:]port [--http [host:]port --resources DIR]
```

## Configuration & Environment Variables

| Variable | Default | Purpose |
| -------- | ------- | ------- |
| `AIRBUS_URL` | `127.0.0.1:9097` | Client TCP `host:port` |
| `AIRBUS_HOST` | `127.0.0.1` | Fallback host |
| `AIRBUS_PORT` | `9097` | Fallback port |
| `AIRBUS_BIN` | `out/airbus` | Binary path for tests / helpers |
| `AIRBUS_HTTP_URL` | `0.0.0.0:9098` | Optional HTTP debug listener (local make run) |
| `AIRBUS_RESOURCES` | `resources/ui` | HTTP static assets |

## Python Client

```bash
pip install airbus-client
```

```python
from airbus_client import RpcClient

client = RpcClient()
assert client.ping() == "pong"
```

## C++ / PlatformIO Clients

Release assets (on `v*` tags), both built from `client-cpp/`:

- `airbus-client-cpp-vX.Y.Z.zip` — CMake / desktop
- `airbus-client-pio-vX.Y.Z.zip` — same sources + PlatformIO `library.json` for `lib_deps`

```cpp
#include <airbus/client.hpp>

airbus::RpcClient client;
assert(client.ping() == "pong");

airbus::Queue queue("tasks");
queue.create(airbus::QueueMode::Fifo);
queue.post({{"action", "build"}});
```

## Detailed Documentation

- [Architecture & Layering](architecture.md)
- [Protocol & Contracts](protocol.md)
- [Queues & Dispatch](queues-and-dispatch.md)
