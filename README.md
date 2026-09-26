# Airbus

JSON-RPC event bus: a Rust daemon plus language clients (Python on PyPI; C++ / PlatformIO via GitHub Release assets).

This repository is the **canonical home** for Airbus (daemon, schema, client, debug UI,
and container image). Other projects consume the published client and/or run the
container — they do not need to vendor this tree.

```
.
├── schema/              # payload SOT (JSON Schema + OpenRPC catalog)
├── scripts/             # codegen + protocol boundary checks + release packing
├── resources/ui/        # static debug UI (--http --resources)
├── src/                 # Rust daemon (app / io / proto / runtime)
├── tests/               # Rust unit + JS protocol tests
├── client-py/           # Python airbus-client (publishable package)
├── client-cpp/          # C++ client (CMake; source of both release zips)
├── packaging/           # Release packaging templates (e.g. PlatformIO library.json)
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

## C++ / PlatformIO clients (GitHub Release assets)

On each `v*` tag, CI attaches two zips built from **`client-cpp/`** (PlatformIO is
only a packaging wrapper — same sources plus `library.json`):

| Asset | Use for |
| ----- | ------- |
| `airbus-client-cpp-vX.Y.Z.zip` | Desktop CMake / FetchContent |
| `airbus-client-pio-vX.Y.Z.zip` | PlatformIO `lib_deps` |

```bash
# Desktop
make pack-client-cpp   # → dist/airbus-client-cpp-v*.zip + dist/airbus-client-pio-v*.zip
```

```ini
; platformio.ini
lib_deps =
  https://github.com/ghhwer/airbus/releases/download/v0.1.0/airbus-client-pio-v0.1.0.zip
```

```cpp
#include <airbus/client.hpp>
airbus::RpcClient client;
assert(client.ping() == "pong");
```

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
                # scripts → client-cpp/.../payloads.hpp + contracts
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
| `attach_listener` / `detach_listener` / `list_listeners` | Push delivery |
| `queue_ready` | Mode-aware readiness |

Preferred application API: `Queue` (`create` / `attach` / `post` / `is_ready`).
