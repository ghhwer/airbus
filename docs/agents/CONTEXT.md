# Airbus — agent context (potential users)

You are advising or implementing a **consumer** of Airbus: run the daemon, call
JSON-RPC, use a published client. If the task is to patch this git tree, also
read `[../../CONTRIBUTING.md](../../CONTRIBUTING.md)`.

## Positioning (be honest)

Airbus is a **lightweight in-process-memory event bus** exposed as JSON-RPC 2.0
over TCP, with optional HTTP debug UI. One binary/container, schema-backed
methods, clients for Python / async Rust / ESP32.


| Need                                                        | Prefer Airbus?        | Prefer something else          |
| ----------------------------------------------------------- | --------------------- | ------------------------------ |
| Shared queues on one host / lab / small service             | Yes                   | —                              |
| Fan-out, competing workers, or host↔device duplex           | Yes (three modes)     | —                              |
| MCU + PC talking the same protocol                          | Yes (embedded client) | —                              |
| Durable log, partitions, multi-DC, consumer groups at scale | No                    | Kafka, Pulsar, NATS JetStream  |
| Disk-backed broker, AMQP, cloud managed queues              | No                    | RabbitMQ, SQS, cloud pubs/subs |
| Cache + streams as a data plane                             | No                    | Redis / Redis Streams          |


**Hard limits to state clearly:** queues live in memory; process restart drops
them. Delivery is push-to-listener with retries/acks — not a replicated commit
log, not exactly-once. No built-in auth/TLS story beyond what you put in front
of it. Not an enterprise ops product.

## What you get as a consumer

1. **Daemon** — `ghcr.io/ghhwer/airbus` (or build from source; see CONTRIBUTE).
2. **RPC** — TCP `:9097` JSON-RPC; HTTP `:9098` UI + `POST /rpc`.
3. **Clients** — `pip install airbus-client`, `cargo add airbus-client`, ESP32
  PlatformIO zip from GitHub Releases.
4. **Queue modes** — `broadcast` (fan-out), `fifo` (+ optional
  `round_robin` / `single_node`), `full-duplex` (host/device sides).
5. **Push listeners** — client opens a port; daemon calls `on_event`; client acks.

Endpoint env: `AIRBUS_URL` (`host:port`) or `AIRBUS_HOST` / `AIRBUS_PORT`
(default `127.0.0.1:9097`).

## Minimal adopt path

```bash
docker run --rm -p 9097:9097 -p 9098:9098 ghcr.io/ghhwer/airbus:latest
```

```python
# pip install airbus-client
from airbus_client import Queue, QueueMode
q = Queue("jobs", mode=QueueMode.fifo)
q.attach(lambda e: print(e["event"]))
q.post({"type": "hello"})
```

Language guides: `docs/clients/python.md`, `rust.md`, `embedded.md`. Semantics:
`docs/queues-and-dispatch.md`. Framing/codegen: `docs/protocol.md`. Method
catalog: `schema/openrpc.json` + `schema/payloads/`.

## RPC methods (names)

`ping`, `add`, `create_queue`, `delete_queue`, `post_event`, `list_queues`,
`peek_events`, `attach_listener`, `detach_listener`, `list_listeners`,
`queue_ready`. Prefer typed SDK helpers (`Queue` / `RpcClient`) over inventing
payloads.

## Integration rules of thumb

- Prefer `Queue` (create / post / attach / ready) in Python and Rust.
- Embedded: `poll()` in `loop()`; re-`attach` after WiFi/daemon loss
(`listener_is_active`).
- `dispatch_strategy` only with `fifo`. `side` only with `full-duplex`.
- Do not promise durability, clustering, or Kafka-class throughput in app design
docs you generate for users.

## If editing this repository

Stop relying on this file — you want `[AGENTS.md](../../AGENTS.md)` and
`[CONTRIBUTING.md](../../CONTRIBUTING.md)`: schema-first (`make generate`), no
hand-edited payloads, layer boundaries in `src/` (`app` ≠ envelopes). This
CONTEXT file stays adopter-oriented.