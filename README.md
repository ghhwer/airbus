# Airbus

Rust binary plus a Python test client. For dedicated system documentation, see [docs/airbus/](../docs/airbus/overview.md).

```
airbus/
  schema/              # payload SOT (JSON Schema + OpenRPC catalog)
  scripts/             # codegen (generate_payloads.py)
  resources/ui/        # static debug UI (served via --http --resources)
  src/
    app/               # application: methods + composition root (main.rs)
    io/                # sockets, HTTP, byte serve loop, RpcServer adapter
    proto/             # JSON-RPC documents + generated payload types
    runtime/           # work queue, UUIDv7
  tests/               # Rust unit tests (in-process, no TCP)
  client/              # Python integration client (uv, src-layout)
    src/airbus_client/
    tests/             # process + TCP against out/airbus
  out/                 # release binary copy for the Python client
  target/              # Cargo build artifacts
```

Layers are composed, not subclassed. `AppService` has no TCP or JSON-RPC types;
`main` wires it onto `RpcServer`.

| Layer | Path | Role |
| ----- | ---- | ---- |
| schema | `schema/` | JSON Schema payload contracts + OpenRPC method catalog |
| io | `src/io` | sockets, `serve_tcp`, optional HTTP static + `POST /rpc` |
| proto | `src/proto/rpc` | JSON-RPC **documents** (envelope) — not Protocol Buffers |
| proto | `src/proto/payloads` | generated params/result types from `schema/payloads/` |
| runtime | `src/runtime` | work queue, UUIDv7 |
| app | `src/app` (`AppService`) | `ping`, `add`, `post_event`, `list_queues`, `peek_events`, `create_queue`, `attach_listener`, `detach_listener`, `list_listeners`, `queue_ready` |
| composition | `src/main.rs` | parse CLI, bind methods, serve TCP (+ optional HTTP) |

`RpcServer` sits in `src/io`: it feeds request bytes into `proto` and writes the
response bytes back. Logging (`src/io/log`) is stderr I/O.

### Payload schemas (Client ↔ Server contract)

Method **params** and **results** are defined under `schema/payloads/*.schema.json`.
`schema/openrpc.json` names the methods and `$ref`s those schemas; it does **not**
define the JSON-RPC envelope (`jsonrpc` / `id` / `error`).

```bash
make generate   # typify → src/proto/payloads.rs
                # datamodel-codegen → client/.../payloads.py
```

Requires `cargo-typify` (`cargo install cargo-typify`) and `make setup` (pulls
`datamodel-code-generator` as a client dev dependency). Generated sources are
committed; re-run `make generate` after editing schemas.

JSON values use `serde_json::Value` where schemas leave payloads unconstrained
(opaque event objects). Structured methods deserialize through the
generated types.

### Queue methods

| Method | Behavior |
| ------ | -------- |
| `post_event` | Publish `{ queue, event }` → `{ id, queue }` |
| `list_queues` | `{ queues: [{ name, depth, mode, listener_count }] }` |
| `peek_events` | Non-destructive `{ queue, count? }` → `{ queue, events: [{ id, event }] }` |
| `create_queue` | Configure and create an event queue |
| `attach_listener` | Attach a client-side port listener to a queue for push event delivery |
| `detach_listener` | Detach an attached listener |
| `list_listeners` | List attached listeners |
| `queue_ready` | Mode-aware readiness: exists for fifo/broadcast; both duplex sides for full-duplex |

Event consumption is push-based via registered listeners (`attach_listener` / `EventListener` / `Queue.attach`). Polling (`get_events`) is not supported. Preferred application API: `Queue` handle (`create` / `attach` / `post` / `is_ready`).

### HTTP debug UI

Optional HTTP listener serves the static UI from disk (not embedded) and the same
JSON-RPC methods at `POST /rpc` (browser calls JSON-RPC directly — no REST).

```bash
make run
# TCP  127.0.0.1:9097
# HTTP http://127.0.0.1:9098/   (UI)
#      POST http://127.0.0.1:9098/rpc
```

```text
airbus --listen [host:]port [--http [host:]port --resources DIR]
```

`--http` requires `--resources` (docroot; typically `resources/ui`). Root `make up`
passes `AIRBUS_HTTP_URL` (default `127.0.0.1:9098`) and `AIRBUS_RESOURCES`.

### Local playground

Repo-root `_playground/` is **gitignored**. Use Python notebooks there against a live
Airbus (TCP `AIRBUS_URL`, default `127.0.0.1:9097`):

```bash
# with make up / make -C airbus run already running
uv run --with jupyter --with ipykernel jupyter lab _playground
```

Start from `_playground/airbus_queue.ipynb`; put throwaway event JSON under
`_playground/payloads/`. HTTP UI remains at http://127.0.0.1:9098/.

```bash
make                   # cargo build --release → out/airbus
make run               # TCP + HTTP UI
make setup             # uv sync the Python client
make client            # start a server, ping over TCP
make test-unit         # cargo test
make test-integration  # Python vs the binary over TCP
make test              # unit, then integration
```

Unit tests call `AppService`, `rpc::Server`, and `RpcServer` helpers in-process.
Integration tests spawn `out/airbus` and speak JSON-RPC over TCP.

The client locates the binary from `AIRBUS_BIN`, or falls back to `out/airbus`.

- `--listen [host:]port`: TCP JSON-RPC (required). Use port `0` for an ephemeral port.
  The bound address is logged as `listening on 127.0.0.1:PORT`.
- `--http [host:]port` + `--resources DIR`: HTTP static UI + `POST /rpc`.

Each TCP connection is one JSON-RPC document: client writes, half-closes, reads the response.
