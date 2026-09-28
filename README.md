# Airbus

JSON-RPC event bus: a Rust daemon plus two language clients — Python on PyPI and
Arduino/ESP32 C++ via a GitHub Release zip.

This repository is the **canonical home** for Airbus (daemon, schema, clients, debug UI,
and container image). Other projects consume the published client and/or run the
container — they do not need to vendor this tree.

```
.
├── schema/              # payload SOT (JSON Schema + OpenRPC catalog)
├── scripts/             # codegen + protocol boundary checks + release packing
├── resources/ui/        # static debug UI (--http --resources)
├── src/                 # Rust daemon (app / io / proto / runtime)
├── tests/               # Rust unit + JS protocol tests
├── client-py/           # Python airbus-client (PyPI)
├── client-embedded/     # Arduino/ESP32 client (PlatformIO zip)
├── docs/                # architecture, protocol, queues
├── Dockerfile           # daemon + UI image
└── out/                 # release binary copy (local builds)
```

Docs: [overview](docs/overview.md) · [architecture](docs/architecture.md) ·
[protocol](docs/protocol.md) · [queues](docs/queues-and-dispatch.md)

## Quick start (local)

```bash
make setup    # uv workspace (airbus-client + codegen deps)
make build    # → out/airbus
make run      # TCP 0.0.0.0:9097 + HTTP UI :9098
make test
```

```text
airbus --listen [host:]port [--http [host:]port --resources DIR]
```

## Python client (PyPI)

```bash
pip install airbus-client
```

```python
from airbus_client import RpcClient

client = RpcClient()  # AIRBUS_URL or AIRBUS_HOST / AIRBUS_PORT
assert client.ping() == "pong"
```

Publish (maintainers), after tagging `v*`:

```bash
# Trusted Publisher / uv publish from CI (see .github/workflows/publish-client.yml)
# or locally:
make publish-client   # requires UV_PUBLISH_TOKEN (or equivalent)
```

## Embedded C++ client (GitHub Release)

On each `v*` tag, CI attaches the Arduino/ESP32 PlatformIO zip. Schema validation is
daemon-side only; the package ships generated ArduinoJson typed payloads.

```bash
make pack-client-embedded   # → dist/airbus-client-arduino-esp32-v*.zip
```

```ini
; platformio.ini (ESP32)
lib_deps =
  https://github.com/ghhwer/airbus/releases/download/v0.1.1/airbus-client-arduino-esp32-v0.1.1.zip
```

```cpp
#include <airbus/embedded_rpc.h>
airbus::EmbeddedRpcClient rpc;
String err;
assert(rpc.ping(err));
```

See [`client-embedded/README.md`](client-embedded/README.md).

## Container

```bash
make docker
make docker-run
# or
docker compose up --build
```

Published images (on version tags) go to `ghcr.io/ghhwer/airbus`.

| Port | Role |
| ---- | ---- |
| `9097` | TCP JSON-RPC |
| `9098` | HTTP debug UI + `POST /rpc` |

## Environment

| Variable | Default | Purpose |
| -------- | ------- | ------- |
| `AIRBUS_URL` | `127.0.0.1:9097` | Client TCP endpoint (`host:port`) |
| `AIRBUS_HOST` / `AIRBUS_PORT` | `127.0.0.1` / `9097` | Client fallbacks |
| `AIRBUS_BIN` | `out/airbus` | Path used by tests / helpers |

## Schema / codegen

```bash
make generate   # typify → src/proto/payloads.rs
                # datamodel-codegen → client-py/.../payloads.py
                # scripts → client-embedded/.../payloads.h
```

Requires `cargo-typify` (`cargo install cargo-typify --version 0.8.0`) and `make setup`.
Generated sources are committed; re-run after schema edits.

## Queue methods (summary)

| Method | Behavior |
| ------ | -------- |
| `post_event` | Publish `{ queue, event }` → `{ id, queue }` |
| `list_queues` | Queue inventory |
| `peek_events` | Non-destructive peek |
| `create_queue` | Configure / create a queue |
| `delete_queue` | Delete a queue (and its listeners / events) |
| `attach_listener` / `detach_listener` / `list_listeners` | Push delivery |
| `queue_ready` | Mode-aware readiness |
