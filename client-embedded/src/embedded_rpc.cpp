#include <airbus/embedded_rpc.h>

#include <WiFiClient.h>
#include <lwip/sockets.h>
#include <type_traits>

namespace airbus {

EmbeddedRpcClient::EmbeddedRpcClient(const char *host, uint16_t port,
                                     uint32_t timeout_ms)
    : host_(host ? host : ""), port_(port), timeout_ms_(timeout_ms) {}

void EmbeddedRpcClient::set_endpoint(const char *host, uint16_t port) {
  host_ = host ? host : "";
  port_ = port;
}

void EmbeddedRpcClient::set_timeout_ms(uint32_t timeout_ms) {
  timeout_ms_ = timeout_ms;
}

bool EmbeddedRpcClient::call(const char *method, JsonDocument *params,
                             JsonDocument &result, String &err) {
  if (host_.length() == 0 || method == nullptr) {
    err = "bad_args";
    return false;
  }

  WiFiClient client;
  client.setTimeout(timeout_ms_ / 1000);
  if (!client.connect(host_.c_str(), port_)) {
    err = "connect_failed";
    return false;
  }

  JsonDocument req;
  req["jsonrpc"] = "2.0";
  req["id"] = next_id_++;
  req["method"] = method;
  if (params != nullptr) {
    req["params"] = *params;
  }

  String body;
  serializeJson(req, body);
  body += '\n';
  if (client.print(body) != (int)body.length()) {
    client.stop();
    err = "send_failed";
    return false;
  }
  client.flush();

  int fd = client.fd();
  if (fd >= 0) {
    ::shutdown(fd, SHUT_WR);
  }

  String resp;
  resp.reserve(512);
  const uint32_t start = millis();
  while (millis() - start < timeout_ms_) {
    while (client.available()) {
      const char c = (char)client.read();
      if (c == '\n') {
        goto got_line;
      }
      resp += c;
      if (resp.length() > 4096) {
        client.stop();
        err = "resp_too_large";
        return false;
      }
    }
    if (!client.connected() && !client.available()) {
      break;
    }
    delay(1);
  }
got_line:
  client.stop();

  if (resp.length() == 0) {
    err = "empty_response";
    return false;
  }

  JsonDocument doc;
  if (deserializeJson(doc, resp)) {
    err = "bad_json";
    return false;
  }
  if (doc["error"].is<JsonObject>()) {
    err = doc["error"]["message"] | "rpc_error";
    return false;
  }
  result.clear();
  result.set(doc["result"]);
  return true;
}

template <typename Params, typename Result>
bool EmbeddedRpcClient::invoke_object(const char *method, const Params &params,
                                      Result &result, String &err) {
  JsonDocument pdoc;
  if (!toJson(pdoc.to<JsonObject>(), params, err)) {
    return false;
  }
  JsonDocument rdoc;
  if (!call(method, &pdoc, rdoc, err)) {
    return false;
  }
  if constexpr (std::is_same_v<Result, AddResult> ||
                std::is_same_v<Result, PingResult>) {
    return fromJson(rdoc.as<JsonVariantConst>(), result, err);
  } else {
    if (!rdoc.is<JsonObjectConst>()) {
      err = "bad_result";
      return false;
    }
    return fromJson(rdoc.as<JsonObjectConst>(), result, err);
  }
}

template <typename Result>
bool EmbeddedRpcClient::invoke_no_params(const char *method, Result &result,
                                         String &err) {
  JsonDocument rdoc;
  if (!call(method, nullptr, rdoc, err)) {
    return false;
  }
  if constexpr (std::is_same_v<Result, PingResult> ||
                std::is_same_v<Result, AddResult>) {
    return fromJson(rdoc.as<JsonVariantConst>(), result, err);
  } else {
    if (!rdoc.is<JsonObjectConst>()) {
      err = "bad_result";
      return false;
    }
    return fromJson(rdoc.as<JsonObjectConst>(), result, err);
  }
}

bool EmbeddedRpcClient::ping(String &err) {
  PingResult result;
  if (!invoke_no_params("ping", result, err)) {
    return false;
  }
  if (result != "pong") {
    err = "unexpected_pong";
    return false;
  }
  return true;
}

bool EmbeddedRpcClient::add(const AddParams &params, AddResult &result,
                            String &err) {
  JsonDocument pdoc;
  if (!toJson(pdoc.to<JsonVariant>(), params, err)) {
    return false;
  }
  JsonDocument rdoc;
  if (!call("add", &pdoc, rdoc, err)) {
    return false;
  }
  return fromJson(rdoc.as<JsonVariantConst>(), result, err);
}

bool EmbeddedRpcClient::create_queue(const CreateQueueParams &params,
                                     CreateQueueResult &result, String &err) {
  return invoke_object("create_queue", params, result, err);
}

bool EmbeddedRpcClient::delete_queue(const DeleteQueueParams &params,
                                     DeleteQueueResult &result, String &err) {
  return invoke_object("delete_queue", params, result, err);
}

bool EmbeddedRpcClient::post_event(const PostEventParams &params,
                                   PostEventResult &result, String &err) {
  return invoke_object("post_event", params, result, err);
}

bool EmbeddedRpcClient::list_queues(ListQueuesResult &result, String &err) {
  return invoke_no_params("list_queues", result, err);
}

bool EmbeddedRpcClient::peek_events(const PeekEventsParams &params,
                                    PeekEventsResult &result, String &err) {
  return invoke_object("peek_events", params, result, err);
}

bool EmbeddedRpcClient::attach_listener(const AttachListenerParams &params,
                                        AttachListenerResult &result,
                                        String &err) {
  return invoke_object("attach_listener", params, result, err);
}

bool EmbeddedRpcClient::detach_listener(const DetachListenerParams &params,
                                        DetachListenerResult &result,
                                        String &err) {
  return invoke_object("detach_listener", params, result, err);
}

bool EmbeddedRpcClient::list_listeners(const ListListenersParams &params,
                                       ListListenersResult &result,
                                       String &err) {
  return invoke_object("list_listeners", params, result, err);
}

bool EmbeddedRpcClient::queue_ready(const QueueReadyParams &params,
                                    QueueReadyResult &result, String &err) {
  return invoke_object("queue_ready", params, result, err);
}

bool EmbeddedRpcClient::listener_is_active(const char *queue,
                                           const char *listener_id,
                                           bool &found_active, String &err) {
  if (listener_id == nullptr || listener_id[0] == '\0') {
    found_active = false;
    return true;
  }
  ListListenersParams params;
  if (queue != nullptr && queue[0] != '\0') {
    params.has_queue = true;
    params.queue = queue;
  }
  ListListenersResult result;
  if (!list_listeners(params, result, err)) {
    return false;
  }
  found_active = false;
  for (const Listener &li : result.listeners) {
    if (li.id != listener_id) {
      continue;
    }
    found_active = li.active;
    return true;
  }
  return true;
}

}  // namespace airbus
