# Airbus Overview

## Components

- **Daemon** (`src/`): JSON-RPC TCP server (queues + dispatch).
- **Python / Rust / embedded clients**: published SDKs (PyPI, crates.io, Release zip).
- **Debug UI** (`resources/ui/`): inspect queues and run RPC in the browser.
- **Container**: `[ghcr.io/ghhwer/airbus](https://github.com/ghhwer/airbus/pkgs/container/airbus)`.

```
┌─────────────────────────────────────────────────────────────────┐
│                         Your application                        │
│                                                                 │
│   airbus-client (pip / crates.io / ESP32)                       │
│              ── JSON-RPC 2.0 (TCP :9097) ──▶ Airbus daemon      │
└─────────────────────────────────────────────────────────────────┘
```

## Capabilities

1. Named queues with three modes ([Queues & Dispatch](queues-and-dispatch.md)):
  - `broadcast` — every listener gets every event
  - `fifo` — one listener per event; optional `round_robin` / `single_node`
  - `full-duplex` — host ↔ device cross-route (`side` required)
2. UUIDv7 event IDs (time-ordered)
3. Push `on_event` delivery with acks, retries, timeouts
4. Non-destructive `peek_events`
5. Schema-first OpenRPC / JSON Schema; generated client types
6. HTTP debug UI + `POST /rpc`

## Quick start (consume)

```bash
docker run --rm -p 9097:9097 -p 9098:9098 ghcr.io/ghhwer/airbus:latest
```

```bash
pip install airbus-client
```

```python
from airbus_client import Queue, QueueMode

q = Queue("jobs", mode=QueueMode.fifo)
q.attach(lambda e: print(e["event"]))
q.post({"type": "hello"})
```

Build/run from this repo: [CONTRIBUTING.md](../CONTRIBUTING.md).

## Environment


| Variable                      | Default              | Purpose                          |
| ----------------------------- | -------------------- | -------------------------------- |
| `AIRBUS_URL`                  | `127.0.0.1:9097`     | Client TCP `host:port`           |
| `AIRBUS_HOST` / `AIRBUS_PORT` | `127.0.0.1` / `9097` | Fallbacks                        |
| `AIRBUS_BIN`                  | `out/airbus`         | Binary for tests / helpers (dev) |


## More docs

- [Client usage](clients/README.md) — [Python](clients/python.md) · [Rust](clients/rust.md) · [Embedded](clients/embedded.md)
- [Queues & Dispatch](queues-and-dispatch.md)
- [Protocol](protocol.md) · [Architecture](architecture.md)
- [AI agents (adopters)](agents/README.md)
- [CONTRIBUTING.md](../CONTRIBUTING.md) — develop and change this tree

