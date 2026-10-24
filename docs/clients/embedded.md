# Embedded C++ client (Arduino / ESP32)

Poll-loop Airbus client with **generated typed payloads** (ArduinoJson). Wire protocol
matches the daemon OpenRPC catalog. Runtime JSON Schema validation stays on the daemon —
this package does not ship schema catalogs.


|           | Python / Rust                  | Embedded                             |
| --------- | ------------------------------ | ------------------------------------ |
| Transport | TCP sockets                    | `WiFiClient` / lwIP                  |
| Events    | background thread / Tokio task | `listener.poll()` in `loop()`        |
| Errors    | exceptions / `Result`          | `bool` + `String& err`               |
| Ship      | PyPI / crates.io               | `airbus-client-arduino-esp32-v*.zip` |


## Install from a GitHub Release

On each `v*` tag, CI attaches the PlatformIO zip:

```ini
; platformio.ini (ESP32)
lib_deps =
  https://github.com/ghhwer/airbus/releases/download/v0.1.1/airbus-client-arduino-esp32-v0.1.1.zip
```

Dependency: [ArduinoJson](https://arduinojson.org/) v7 (declared in `library.json`).

From this repo, pack locally with `make pack-client-embedded`. After schema edits:

```bash
make generate   # → client-embedded/include/airbus/payloads.h
```



## Connect and ping

```cpp
#include <airbus/embedded_rpc.h>

airbus::EmbeddedRpcClient rpc("192.168.1.10", 9097);
String err;
if (!rpc.ping(err)) {
  // err holds the failure message
}
```

Or:

```cpp
airbus::EmbeddedRpcClient rpc;
rpc.set_endpoint("192.168.1.10", 9097);
rpc.set_timeout_ms(8000);
```



## Create a queue and publish

```cpp
airbus::CreateQueueParams create;
create.queue = "esp32_jobs";
create.has_mode = true;
create.mode = airbus::QueueMode::Fifo;
create.has_dispatch_strategy = true;
create.dispatch_strategy = airbus::DispatchStrategy::SingleNode;
airbus::CreateQueueResult created;
if (!rpc.create_queue(create, created, err)) {
  // handle err
}

airbus::PostEventParams post;
post.queue = "esp32_jobs";
post.event["hello"] = "from_esp32";
airbus::PostEventResult posted;
rpc.post_event(post, posted, err);
// posted.id is a UUIDv7 string
```

Optional fields use `has_*` flags (ArduinoJson codegen). Queue mode rules:
[Queues & Dispatch](../queues-and-dispatch.md).

## Attach a push listener

Bind a listen port on the MCU, register with Airbus using the device's LAN IP, then
**call** `poll()` **every loop iteration**:

```cpp
#include <WiFi.h>
#include <airbus/embedded_listener.h>

airbus::EmbeddedEventListener listener;

void on_event(const airbus::ListenerEventParams &params, void * /*user*/) {
  // params.id / params.queue / params.event (JsonObject)
  // ack is sent after this callback returns
}

void setup() {
  // ... WiFi.begin ...
  listener.set_on_event(on_event);
  listener.begin(19097);

  airbus::AttachListenerParams attach;
  attach.queue = "esp32_jobs";
  attach.port = 19097;
  attach.has_host = true;
  attach.host = WiFi.localIP().toString();
  attach.has_max_retries = true;
  attach.max_retries = 3;
  attach.has_exhaustion_timeout_ms = true;
  attach.exhaustion_timeout_ms = 30000;
  airbus::AttachListenerResult attached;
  rpc.attach_listener(attach, attached, err);
  // keep attached.listener_id for health checks
}

void loop() {
  listener.poll();
}
```

### Health / re-attach

WiFi drops and daemon restarts are common on MCUs. Periodically verify the listener is
still registered:

```cpp
bool active = false;
if (!rpc.listener_is_active("esp32_jobs", listener_id.c_str(), active, err) || !active) {
  // create_queue (idempotent) + attach_listener again
}
```

See `client-embedded/examples/esp32_listener/` for a full sketch that creates a fifo
`single_node` queue, posts once, attaches, and re-attaches every 15s.

## Method surface

Typed helpers: `ping`, `add`, `create_queue`, `delete_queue`, `post_event`, `list_queues`,
`peek_events`, `attach_listener`, `detach_listener`, `list_listeners`, `queue_ready`,
plus `listener_is_active`. Raw escape hatch: `call(method, params, result, err)`.

## Design notes and limitations

1. **No schema validation on device** — trust the daemon; field names come from codegen.
2. **Poll, don't thread** — avoids fighting AsyncTCP / `WiFiServer` stacks.
3. **Ack after callback** — reply `{status:"ok"}` once the app callback returns.
4. **Framing** — request: JSON + optional `\n` + half-close; dial-back: JSON until `\n` or EOF.

Package README: `[client-embedded/README.md](../../client-embedded/README.md)`.