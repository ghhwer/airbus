# Airbus

Rust binary plus a Python test client.

```
airbus/
  schema/              # payload SOT (JSON Schema + OpenRPC catalog)
  scripts/             # codegen (generate_payloads.py)
  src/
    app/               # application: methods + composition root (main.rs)
    io/                # sockets, byte serve loop, RpcServer adapter
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
| io | `src/io` | sockets, `serve_tcp`; connection GC (planned) |
| proto | `src/proto/rpc` | JSON-RPC **documents** (envelope) — not Protocol Buffers |
| proto | `src/proto/payloads` | generated params/result types from `schema/payloads/` |
| runtime | `src/runtime` | work queue, UUIDv7 |
| app | `src/app` (`AppService`) | `ping`, `add`, `post_event`, `get_events` |
| composition | `src/main.rs` | parse `--listen`, bind methods, serve TCP |

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

```bash
make                   # cargo build --release → out/airbus
make run               # listen on 127.0.0.1:9097
make setup             # uv sync the Python client
make client            # start a server, ping over TCP
make test-unit         # cargo test
make test-integration  # Python vs the binary over TCP
make test              # unit, then integration
```

Unit tests call `AppService`, `rpc::Server`, and `RpcServer` helpers in-process.
Integration tests spawn `out/airbus` and speak JSON-RPC over TCP.

The client locates the binary from `AIRBUS_BIN`, or falls back to `out/airbus`.

- `--listen [host:]port`: TCP server (required). Use port `0` for an ephemeral port.
  The bound address is logged as `listening on 127.0.0.1:PORT`.

Each TCP connection is one JSON-RPC document: client writes, half-closes, reads the response.
