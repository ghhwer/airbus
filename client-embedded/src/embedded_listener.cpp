#include <airbus/embedded_listener.h>

#include <WiFi.h>
#include <errno.h>
#include <fcntl.h>

namespace airbus {
namespace {

bool read_body(int fd, String &out, size_t max_body, uint32_t timeout_ms) {
  out = "";
  out.reserve(256);
  const uint32_t start = millis();
  while (millis() - start < timeout_ms) {
    char c;
    const int n = ::recv(fd, &c, 1, 0);
    if (n == 1) {
      if (c == '\n') {
        return true;
      }
      out += c;
      if (out.length() > max_body) {
        return false;
      }
      continue;
    }
    if (n == 0) {
      return out.length() > 0;
    }
    if (errno == EAGAIN || errno == EWOULDBLOCK) {
      delay(1);
      continue;
    }
    return out.length() > 0;
  }
  return out.length() > 0;
}

}  // namespace

void EmbeddedEventListener::set_on_event(EmbeddedOnEventFn cb, void *user_data) {
  on_event_ = cb;
  user_data_ = user_data;
}

bool EmbeddedEventListener::begin(uint16_t port) {
  close();
  port_ = port;

  listen_fd_ = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
  if (listen_fd_ < 0) {
    return false;
  }

  int yes = 1;
  ::setsockopt(listen_fd_, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes));

  sockaddr_in addr {};
  addr.sin_family = AF_INET;
  addr.sin_port = htons(port_);
  addr.sin_addr.s_addr = htonl(INADDR_ANY);

  if (::bind(listen_fd_, reinterpret_cast<sockaddr *>(&addr), sizeof(addr)) !=
      0) {
    close();
    return false;
  }
  if (::listen(listen_fd_, 4) != 0) {
    close();
    return false;
  }

  const int flags = ::fcntl(listen_fd_, F_GETFL, 0);
  ::fcntl(listen_fd_, F_SETFL, flags | O_NONBLOCK);
  return true;
}

void EmbeddedEventListener::close() {
  if (listen_fd_ >= 0) {
    ::close(listen_fd_);
    listen_fd_ = -1;
  }
}

void EmbeddedEventListener::handle_client(int fd, const sockaddr_in &peer) {
  (void)peer;
  const int flags = ::fcntl(fd, F_GETFL, 0);
  ::fcntl(fd, F_SETFL, flags & ~O_NONBLOCK);

  String body;
  if (!read_body(fd, body, max_body_, 5000) || body.length() == 0) {
    ::close(fd);
    return;
  }

  JsonDocument doc;
  if (deserializeJson(doc, body)) {
    ::close(fd);
    return;
  }

  const char *method = doc["method"] | "";
  bool is_notification = true;
  JsonDocument resp;
  resp["jsonrpc"] = "2.0";
  if (!doc["id"].isNull() || doc["id"].is<int>() ||
      doc["id"].is<const char *>()) {
    resp["id"] = doc["id"];
    is_notification = false;
  }

  if (strcmp(method, "on_event") != 0) {
    if (!is_notification) {
      resp["error"]["code"] = -32601;
      resp["error"]["message"] = "method not found";
      String out;
      serializeJson(resp, out);
      ::send(fd, out.c_str(), out.length(), 0);
    }
    ::close(fd);
    return;
  }

  ListenerEventParams params;
  String err;
  if (!doc["params"].is<JsonObjectConst>() ||
      !fromJson(doc["params"].as<JsonObjectConst>(), params, err)) {
    if (!is_notification) {
      resp["error"]["code"] = -32602;
      resp["error"]["message"] = err.length() ? err.c_str() : "invalid params";
      String out;
      serializeJson(resp, out);
      ::send(fd, out.c_str(), out.length(), 0);
    }
    ::close(fd);
    return;
  }

  if (on_event_) {
    on_event_(params, user_data_);
  }

  if (!is_notification) {
    ListenerEventResult acknowledgment;
    acknowledgment.status = Status::Ok;
    JsonObject result = resp["result"].to<JsonObject>();
    if (!toJson(result, acknowledgment, err)) {
      resp.remove("result");
      resp["error"]["code"] = -32603;
      resp["error"]["message"] = "ack encode failed";
    }
    String out;
    serializeJson(resp, out);
    ::send(fd, out.c_str(), out.length(), 0);
  }
  ::close(fd);
}

void EmbeddedEventListener::poll(int max_accepts) {
  if (listen_fd_ < 0) {
    return;
  }
  for (int i = 0; i < max_accepts; i++) {
    sockaddr_in peer {};
    socklen_t plen = sizeof(peer);
    const int fd =
        ::accept(listen_fd_, reinterpret_cast<sockaddr *>(&peer), &plen);
    if (fd < 0) {
      break;
    }
    handle_client(fd, peer);
  }
}

}  // namespace airbus
