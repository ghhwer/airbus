# airbus-client (Arduino / ESP32)

Poll-loop Airbus client for MCUs with **generated typed payloads** (ArduinoJson).
Wire protocol matches the daemon OpenRPC catalog. Runtime JSON Schema validation is
daemon-side only — this package does not ship schema catalogs.

| | Python (`client-py`) | Embedded (this package) |
|--|----------------------|-------------------------|
| JSON | pydantic typed payloads | ArduinoJson typed payloads (`payloads.h`) |
| Role | Full client (publish + listen) | Full typed method surface |
| Transport | TCP sockets | `WiFiClient` / lwIP |
| Events | background thread listener | `poll()` in `loop()` |
| Errors | exceptions | `bool` + `String& err` |
| Ship | PyPI `airbus-client` | `airbus-client-arduino-esp32-v*.zip` |

## Install from a GitHub Release

```ini
; platformio.ini
lib_deps =
  https://github.com/ghhwer/airbus/releases/download/v0.1.0/airbus-client-arduino-esp32-v0.1.0.zip
```

Dependency: [ArduinoJson](https://arduinojson.org/) v7 (declared in `library.json`).

Regenerate types after schema edits (from the airbus repo):

```bash
make generate   # → client-embedded/include/airbus/payloads.h
```

## Usage

```cpp
#include <airbus/embedded_rpc.h>
#include <airbus/embedded_listener.h>

airbus::EmbeddedRpcClient rpc("airbus.example", 9097);
String err;
rpc.ping(err);

airbus::CreateQueueParams create;
create.queue = "jobs";
create.has_mode = true;
create.mode = airbus::QueueMode::Fifo;
airbus::CreateQueueResult created;
rpc.create_queue(create, created, err);

airbus::EmbeddedEventListener listener;
listener.begin(19097);
listener.set_on_event([](const airbus::ListenerEventParams& p, void*) {
  // handle p.id / p.event; ack is sent after this callback returns
}, nullptr);

airbus::AttachListenerParams attach;
attach.queue = "jobs";
attach.port = 19097;
attach.has_host = true;
attach.host = WiFi.localIP().toString();
airbus::AttachListenerResult attached;
rpc.attach_listener(attach, attached, err);

void loop() {
  listener.poll();
}
```

See `examples/esp32_listener/` for a minimal sketch (includes `post_event`).

## Design notes

1. **No schema validation on device** — trust the daemon; types come from codegen.
2. **Poll, don't thread** — raw lwIP listen socket + `poll()` avoids conflicts with
   AsyncTCP / `WiFiServer` stacks.
3. **Ack after callback** — reply `{status:"ok"}` once the app callback returns.
4. **Framing** — request: JSON + optional `\n` + half-close; dial-back body: JSON
   until `\n` or EOF.
5. **Health** — periodically call `listener_is_active` and re-`attach` if missing /
   `active:false`.

## Limitations (v1)

- **No schema validation on device.** OpenRPC / JSON Schema catalogs are not shipped.
  The daemon remains the runtime validator; `make generate` keeps typed field names
  in sync with `schema/`.
- ArduinoJson still heap-allocates `JsonDocument` / `String` — dropping nlohmann +
  embedded schema blobs is what fixed ESP32 OOM, not a zero-heap design.

## Method surface

Typed helpers: `ping`, `add`, `create_queue`, `post_event`, `list_queues`,
`peek_events`, `attach_listener`, `detach_listener`, `list_listeners`, `queue_ready`,
plus convenience `listener_is_active`. Raw `call()` for escape hatches.
