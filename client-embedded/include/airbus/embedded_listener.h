#pragma once

/**
 * Poll-based Airbus dial-back listener (on_event).
 * No std::thread; call poll() from the app loop.
 */

#include <Arduino.h>
#include <ArduinoJson.h>
#include <lwip/sockets.h>

#include <airbus/payloads.h>

namespace airbus {

/** Called for each on_event. Ack is sent after the callback returns. */
using EmbeddedOnEventFn = void (*)(const ListenerEventParams &params,
                                   void *user_data);

class EmbeddedEventListener {
 public:
  EmbeddedEventListener() = default;
  ~EmbeddedEventListener() { close(); }

  EmbeddedEventListener(const EmbeddedEventListener &) = delete;
  EmbeddedEventListener &operator=(const EmbeddedEventListener &) = delete;

  void set_on_event(EmbeddedOnEventFn cb, void *user_data = nullptr);
  void set_max_body(size_t max_body) { max_body_ = max_body; }

  /** Bind 0.0.0.0:port (non-blocking accept). */
  bool begin(uint16_t port);
  void close();

  bool listening() const { return listen_fd_ >= 0; }
  uint16_t port() const { return port_; }
  int fd() const { return listen_fd_; }

  /** Accept pending dial-backs and dispatch on_event. Call from loop(). */
  void poll(int max_accepts = 4);

 private:
  void handle_client(int fd, const sockaddr_in &peer);

  int listen_fd_ = -1;
  uint16_t port_ = 0;
  size_t max_body_ = 4096 + 256;
  EmbeddedOnEventFn on_event_ = nullptr;
  void *user_data_ = nullptr;
};

}  // namespace airbus
