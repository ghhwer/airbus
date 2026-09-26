#pragma once

#include <functional>
#include <memory>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

#include "airbus/payloads.hpp"
#include "airbus/rpc.hpp"

namespace airbus {

/** Named queue handle — create / attach / post / readiness over Airbus RPC. */
class Queue {
 public:
  explicit Queue(std::string name, RpcClient* client = nullptr,
                 std::optional<DuplexSide> side = std::nullopt,
                 std::string listen_host = "127.0.0.1");

  const std::string& name() const { return name_; }
  std::optional<DuplexSide> side() const { return side_; }

  bool create(QueueMode mode,
              std::optional<DispatchStrategy> dispatch_strategy = std::nullopt);

  EventListener& attach(
      std::function<void(const nlohmann::json&)> on_event,
      std::optional<std::int64_t> max_events = 100,
      std::optional<std::int64_t> exhaustion_timeout_ms = std::nullopt,
      std::optional<std::int64_t> max_retries = std::nullopt);

  EventListener* try_attach(
      std::function<void(const nlohmann::json&)> on_event,
      std::optional<std::int64_t> max_events = 100,
      std::optional<std::int64_t> exhaustion_timeout_ms = std::nullopt,
      std::optional<std::int64_t> max_retries = std::nullopt);

  bool post(const nlohmann::json& event);
  bool is_ready();
  void close();

 private:
  std::unique_ptr<RpcClient> owned_client_;
  RpcClient* client_;
  std::string name_;
  std::optional<DuplexSide> side_;
  std::string listen_host_;
  std::unique_ptr<EventListener> listener_;
};

}  // namespace airbus
