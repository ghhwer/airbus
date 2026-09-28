/**
 * Minimal Airbus ESP32 listener sketch.
 *
 * Fill WIFI_SSID / WIFI_PASS / AIRBUS_HOST, then call listener.poll() from loop().
 * For PlatformIO, point lib_deps at airbus-client-arduino-esp32-v*.zip.
 */

#include <WiFi.h>
#include <airbus/embedded_rpc.h>
#include <airbus/embedded_listener.h>

#ifndef WIFI_SSID
#define WIFI_SSID "your-ssid"
#endif
#ifndef WIFI_PASS
#define WIFI_PASS "your-pass"
#endif
#ifndef AIRBUS_HOST
#define AIRBUS_HOST "192.168.1.10"
#endif
#ifndef AIRBUS_PORT
#define AIRBUS_PORT 9097
#endif
#ifndef LISTEN_PORT
#define LISTEN_PORT 19097
#endif
#ifndef QUEUE_NAME
#define QUEUE_NAME "esp32_jobs"
#endif

static airbus::EmbeddedRpcClient rpc;
static airbus::EmbeddedEventListener listener;
static String listener_id;
static uint32_t last_health_ms = 0;

static void on_event(const airbus::ListenerEventParams &params, void * /*user*/) {
  Serial.printf("on_event id=%s queue=%s\n", params.id.c_str(),
                params.queue.c_str());
  serializeJson(params.event, Serial);
  Serial.println();
}

static bool ensure_attached(String &err) {
  if (listener_id.length() > 0) {
    bool active = false;
    if (rpc.listener_is_active(QUEUE_NAME, listener_id.c_str(), active, err) &&
        active) {
      return true;
    }
    listener_id = "";
  }

  airbus::CreateQueueParams create;
  create.queue = QUEUE_NAME;
  create.has_mode = true;
  create.mode = airbus::QueueMode::Fifo;
  create.has_dispatch_strategy = true;
  create.dispatch_strategy = airbus::DispatchStrategy::SingleNode;
  airbus::CreateQueueResult created;
  if (!rpc.create_queue(create, created, err)) {
    return false;
  }

  airbus::AttachListenerParams attach;
  attach.queue = QUEUE_NAME;
  attach.port = LISTEN_PORT;
  attach.has_host = true;
  attach.host = WiFi.localIP().toString();
  attach.has_max_retries = true;
  attach.max_retries = 3;
  attach.has_exhaustion_timeout_ms = true;
  attach.exhaustion_timeout_ms = 30000;
  airbus::AttachListenerResult attached;
  if (!rpc.attach_listener(attach, attached, err)) {
    return false;
  }
  listener_id = attached.listener_id;
  return listener_id.length() > 0;
}

void setup() {
  Serial.begin(115200);
  WiFi.mode(WIFI_STA);
  WiFi.begin(WIFI_SSID, WIFI_PASS);
  while (WiFi.status() != WL_CONNECTED) {
    delay(200);
  }

  rpc.set_endpoint(AIRBUS_HOST, AIRBUS_PORT);
  listener.set_on_event(on_event);
  if (!listener.begin(LISTEN_PORT)) {
    Serial.println("listen begin failed");
    return;
  }

  String err;
  if (!rpc.ping(err)) {
    Serial.printf("ping failed: %s\n", err.c_str());
  }

  // Optional publish (full typed post_event parity).
  airbus::PostEventParams post;
  post.queue = QUEUE_NAME;
  post.event["hello"] = "from_esp32";
  airbus::PostEventResult posted;
  if (!rpc.post_event(post, posted, err)) {
    Serial.printf("post_event (optional) failed: %s\n", err.c_str());
  } else {
    Serial.printf("posted id=%s\n", posted.id.c_str());
  }

  if (!ensure_attached(err)) {
    Serial.printf("attach failed: %s\n", err.c_str());
  } else {
    Serial.printf("attached listener_id=%s\n", listener_id.c_str());
  }
}

void loop() {
  listener.poll();

  if (millis() - last_health_ms > 15000) {
    last_health_ms = millis();
    String err;
    if (!ensure_attached(err)) {
      Serial.printf("re-attach failed: %s\n", err.c_str());
    }
  }
}
