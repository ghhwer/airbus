#pragma once

#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <thread>
#include <vector>

#include <nlohmann/json.hpp>

#include "airbus/endpoint.hpp"
#include "airbus/error.hpp"
#include "airbus/payloads.hpp"

namespace airbus {

class RpcClient;

/** Internal batch accumulator (defined in rpc.cpp). */
struct BatchState;

class EventListener {
 public:
  EventListener(RpcClient& client, std::string queue,
                std::function<void(const nlohmann::json&)> on_event,
                std::string host = "127.0.0.1",
                std::optional<std::int64_t> exhaustion_timeout_ms = std::nullopt,
                std::optional<std::int64_t> max_retries = std::nullopt,
                std::optional<std::int64_t> max_events = std::nullopt,
                std::optional<DuplexSide> side = std::nullopt);
  ~EventListener();

  EventListener(const EventListener&) = delete;
  EventListener& operator=(const EventListener&) = delete;

  int port() const { return port_; }
  const std::optional<std::string>& listener_id() const { return listener_id_; }
  const std::vector<nlohmann::json>& events() const { return events_; }

  EventListener& start();
  void close();

 private:
  void serve();
  void receive_event(const nlohmann::json& event);

  RpcClient* client_;
  std::string queue_;
  std::function<void(const nlohmann::json&)> on_event_;
  std::string host_;
  std::optional<std::int64_t> exhaustion_timeout_ms_;
  std::optional<std::int64_t> max_retries_;
  std::optional<std::int64_t> max_events_;
  std::optional<DuplexSide> side_;

  int server_fd_ = -1;
  int port_ = 0;
  bool running_ = false;
  std::optional<std::string> listener_id_;
  std::vector<nlohmann::json> events_;
  std::unique_ptr<std::thread> thread_;
};

class RpcClient {
 public:
  explicit RpcClient(std::string host = {}, int port = -1,
                     double timeout_seconds = 2.0);
  ~RpcClient();

  static RpcClient from_url(const std::string& url, double timeout_seconds = 2.0);

  const std::string& host() const { return host_; }
  int port() const { return port_; }

  /** Defer ``call`` / ``notify`` into one JSON-RPC batch until the guard is destroyed. */
  class BatchGuard {
   public:
    explicit BatchGuard(RpcClient& client);
    ~BatchGuard();
    BatchGuard(const BatchGuard&) = delete;
    BatchGuard& operator=(const BatchGuard&) = delete;

   private:
    RpcClient& client_;
  };

  BatchGuard batch();

  nlohmann::json call(const std::string& method,
                      const nlohmann::json& params = nullptr,
                      const nlohmann::json& id = 1);

  void notify(const std::string& method, const nlohmann::json& params = nullptr);

  PingResult ping();
  AddResult add(const AddParams& params);
  CreateQueueResult create_queue(const CreateQueueParams& params);
  PostEventResult post_event(const PostEventParams& params);
  ListQueuesResult list_queues();
  PeekEventsResult peek_events(const PeekEventsParams& params);
  AttachListenerResult attach_listener(const AttachListenerParams& params);
  DetachListenerResult detach_listener(const DetachListenerParams& params);
  ListListenersResult list_listeners(
      const std::optional<ListListenersParams>& params = std::nullopt);
  QueueReadyResult queue_ready(const QueueReadyParams& params);

  std::unique_ptr<EventListener> listen(
      const std::string& queue,
      std::function<void(const nlohmann::json&)> on_event = {},
      const std::string& host = "127.0.0.1",
      std::optional<std::int64_t> exhaustion_timeout_ms = std::nullopt,
      std::optional<std::int64_t> max_retries = std::nullopt,
      std::optional<DuplexSide> side = std::nullopt,
      std::optional<std::int64_t> max_events = std::nullopt);

  nlohmann::json raw(const nlohmann::json& payload);
  nlohmann::json raw_text(const std::string& text);

 private:
  friend class EventListener;
  friend class BatchGuard;

  template <typename T>
  T invoke(const std::string& method, const nlohmann::json& params);

  std::string host_;
  int port_;
  double timeout_seconds_;
  std::unique_ptr<BatchState> batch_;
};

}  // namespace airbus
