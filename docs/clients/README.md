# Airbus clients

Language SDKs for the Airbus JSON-RPC event bus (TCP `:9097` by default). The daemon is
distributed separately — install a client, point it at a running Airbus, then publish and
listen.


| Client             | Package                     | Docs                       |
| ------------------ | --------------------------- | -------------------------- |
| Python             | `airbus-client` (PyPI)      | [python.md](python.md)     |
| Rust (async Tokio) | `airbus-client` (crates.io) | [rust.md](rust.md)         |
| Arduino / ESP32    | GitHub Release zip          | [embedded.md](embedded.md) |


## Shared concepts

- **Endpoint**: `AIRBUS_URL` (`host:port`), or `AIRBUS_HOST` / `AIRBUS_PORT` (default
`127.0.0.1:9097`).
- **Queue handle**: thin `Queue` / typed RPC wrappers for create, post, attach, readiness.
- **Raw RPC**: typed `*Params` / `*Result` (or ArduinoJson structs) for every OpenRPC method.
- **Push delivery**: clients open a listen port; the daemon dials back with `on_event`.

Queue modes and dispatch rules: [Queues & Dispatch](../queues-and-dispatch.md).
Framing / codegen: [Protocol](../protocol.md). Methods & payloads: `[schema/openrpc.json](../../schema/openrpc.json)`.

## Run the daemon first

```bash
docker run --rm -p 9097:9097 -p 9098:9098 ghcr.io/ghhwer/airbus:latest
# RPC :9097 — UI http://127.0.0.1:9098/
```

From this repo (contributors): see [CONTRIBUTING.md](../../CONTRIBUTING.md).