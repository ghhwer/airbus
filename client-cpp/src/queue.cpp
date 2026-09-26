#include "airbus/queue.hpp"

#include <string>

#include "airbus/error.hpp"

namespace airbus {

Queue::Queue(std::string name, RpcClient* client, std::optional<DuplexSide> side,
             std::string listen_host)
    : name_(std::move(name)),
      side_(side),
      listen_host_(std::move(listen_host)) {
  if (client != nullptr) {
    client_ = client;
  } else {
    owned_client_ = std::make_unique<RpcClient>();
    client_ = owned_client_.get();
  }
}

bool Queue::create(QueueMode mode,
                   std::optional<DispatchStrategy> dispatch_strategy) {
  CreateQueueParams params;
  params.queue = name_;
  params.mode = mode;
  params.dispatch_strategy = dispatch_strategy;
  return client_->create_queue(params).created;
}

EventListener& Queue::attach(std::function<void(const nlohmann::json&)> on_event,
                             std::optional<std::int64_t> max_events,
                             std::optional<std::int64_t> exhaustion_timeout_ms,
                             std::optional<std::int64_t> max_retries) {
  if (listener_) {
    close();
  }
  listener_ = client_->listen(name_, std::move(on_event), listen_host_,
                              exhaustion_timeout_ms, max_retries, side_,
                              max_events);
  listener_->start();
  return *listener_;
}

EventListener* Queue::try_attach(
    std::function<void(const nlohmann::json&)> on_event,
    std::optional<std::int64_t> max_events,
    std::optional<std::int64_t> exhaustion_timeout_ms,
    std::optional<std::int64_t> max_retries) {
  try {
    return &attach(std::move(on_event), max_events, exhaustion_timeout_ms,
                   max_retries);
  } catch (const RpcError& exc) {
    const std::string message = exc.what();
    if (exc.code() == -32602 &&
        message.find("does not exist") != std::string::npos) {
      return nullptr;
    }
    throw;
  }
}

bool Queue::post(const nlohmann::json& event) {
  try {
    PostEventParams params;
    params.queue = name_;
    params.event = event;
    params.side = side_;
    client_->post_event(params);
    return true;
  } catch (const RpcError& exc) {
    const std::string message = exc.what();
    if (exc.code() == -32602 &&
        message.find("does not exist") != std::string::npos) {
      return false;
    }
    throw;
  }
}

bool Queue::is_ready() {
  QueueReadyParams params;
  params.queue = name_;
  return client_->queue_ready(params).ready;
}

void Queue::close() {
  if (listener_) {
    listener_->close();
    listener_.reset();
  }
}

}  // namespace airbus
