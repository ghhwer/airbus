# Airbus Protocol & Contracts

Airbus speaks [JSON-RPC 2.0](https://www.jsonrpc.org/specification) — request/response
envelopes, `result` vs `error`, and standard error codes are defined there, not here.

This doc covers what Airbus adds on top: transport framing, schema as SOT, codegen, and
where envelope handling is allowed to live. Method names and payload shapes:
[`schema/openrpc.json`](../schema/openrpc.json) and [`schema/payloads/`](../schema/payloads/).

## Transport Framing

| Transport | Framing Mechanism | Typical Endpoint |
| --------- | ----------------- | ---------------- |
| **TCP** | Newline-delimited JSON (`\n`) | `127.0.0.1:9097` |
| **HTTP** | HTTP POST body (`application/json`) | `POST http://127.0.0.1:9098/rpc` |

On TCP, each request or response is one JSON object terminated by `\n` (`0x0A`). Multiple
requests may be pipelined on a persistent connection. HTTP uses a normal POST body (no
newline framing).

## Enforced Protocol Boundaries

Enforced by `make lint-protocol` (and documented for contributors in CONTRIBUTING /
`docs/agents/`).

1. **Schema Single Source of Truth**:
   - Methods: `schema/openrpc.json`
   - Payloads: `schema/payloads/*.schema.json`
2. **Never edit generated code**:
   - Rust: `src/proto/payloads.rs`, `client-rs/src/payloads.rs` (typify)
   - Python: `client-py/src/airbus_client/payloads.py` (datamodel-code-generator)
   - Embedded: `client-embedded/include/airbus/payloads.h` (`scripts/generate_payloads.py`)
3. **Envelope isolation** — construct/inspect `jsonrpc` / `id` / `method` / `params` /
   `result` / `error` only in:
   - Daemon: `src/proto/rpc.rs`
   - Clients: `client-rs/src/protocol.rs`, `airbus_client/protocol.py`,
     `client-embedded` RPC/listener sources, `resources/ui/protocol.js`
   - Application, transport, and runtime must not touch wire fields directly.
4. **Validation**: the daemon enforces payload types (serde decode). Language clients
   encode/decode generated types and check envelopes only — they do not ship schema
   catalogs.
5. **Freshness**: `make lint-protocol` regenerates into a temp dir and diffs against
   committed files.

### Codegen

```bash
make generate      # update payloads after schema/ changes
make lint-protocol # freshness check
```

## Related

- Method catalog: [`schema/openrpc.json`](../schema/openrpc.json) ·
  [`schema/payloads/`](../schema/payloads/)
- Delivery semantics: [Queues & Dispatch](queues-and-dispatch.md)
- Layer boundaries: [Architecture](architecture.md)
