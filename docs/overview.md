# Airbus Overview

Airbus is a high-performance, lightweight event bus and message queue daemon implemented in
Rust, communicating over JSON-RPC 2.0 with strict cross-language protocol schemas.

## Components

- **Airbus daemon** (`src/`): Rust JSON-RPC TCP server providing event queues and dispatch.
- **Airbus Python client** (`client-py/` → `airbus_client`): publishable SDK for other projects.
- **Airbus embedded client** (`client-embedded/`): Arduino/ESP32 PlatformIO package (ArduinoJson typed payloads, poll-loop).
- **Airbus Debug UI** (`resources/ui/`): Browser interface for queue inspection and RPC execution.
- **Container image** (`Dockerfile`): runs the daemon + UI for deployment.

```
┌─────────────────────────────────────────────────────────────────┐
│                         Your application                        │
│                                                                 │
│   airbus-client (pip / ESP32) ── JSON-RPC 2.0 (TCP :9097) ──▶ Airbus   │
│                                                         daemon  │
│                              ▲                                  │
│                              │ HTTP :9098  Debug UI / POST /rpc │
└─────────────────────────────────────────────────────────────────┘
```

## Key Capabilities

1. **Named Event Queues**: Create and manage queues dynamically with configured delivery modes.
2. **Three Queue Modes** (see [Queues & Dispatch](queues-and-dispatch.md)):
   - `broadcast`: Every attached listener receives every event.
   - `fifo`: Each event goes to one listener; optional `dispatch_strategy` (`round_robin` or exclusive `single_node`).
   - `full-duplex`: Host ↔ device cross-route (`side` required; strategy not allowed).
3. **Time-Ordered Event Identifiers**: Monotonically increasing, time-sortable UUIDv7 IDs.
4. **Push-Based Listener Dispatch**: `on_event` RPC callbacks with acknowledgments.
5. **Configurable Retries & Timeouts**: `max_retries`, `exhaustion_timeout_ms`.
6. **Non-Destructive Peeking**: `peek_events` without advancing consumers.
7. **Strict Contract Boundaries**: Schema-first JSON Schema + OpenRPC; generated Rust/Python/embedded types.
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

## Embedded C++ Client (ESP32)

Release asset on `v*` tags: `airbus-client-arduino-esp32-vX.Y.Z.zip` (`client-embedded/`).

```cpp
#include <airbus/embedded_rpc.h>

airbus::EmbeddedRpcClient rpc;
String err;
assert(rpc.ping(err));
```

## Detailed Documentation

- [Architecture & Layering](architecture.md)
- [Protocol & Contracts](protocol.md)
- [Queues & Dispatch](queues-and-dispatch.md)
