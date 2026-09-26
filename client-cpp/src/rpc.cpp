#include "airbus/rpc.hpp"

#include <exception>
#include <stdexcept>

#include "airbus/detail/transport.hpp"
#include "airbus/protocol.hpp"

namespace airbus {

struct BatchState {
  std::vector<nlohmann::json> requests;
  bool sent = false;
  int next_id = 1;

  void enqueue_notify(const std::string& method, const nlohmann::json& params) {
    if (sent) throw std::runtime_error("cannot enqueue on a sent batch");
    requests.push_back(make_request(method, params, std::nullopt));
  }

  void enqueue_call(const std::string& method, const nlohmann::json& params,
                    const nlohmann::json& id) {
    if (sent) throw std::runtime_error("cannot enqueue on a sent batch");
    requests.push_back(make_request(method, params, id));
  }

  void send(RpcClient& client) {
    if (sent) throw std::runtime_error("batch already sent");
    if (requests.empty()) {
      sent = true;
      return;
    }
    const auto response = client.raw(requests);
    (void)batch_responses(requests, response);
    sent = true;
  }
};

namespace {

thread_local BatchState* active_batch = nullptr;

}  // namespace

RpcClient::BatchGuard::BatchGuard(RpcClient& client) : client_(client) {
  if (active_batch != nullptr) {
    throw std::runtime_error("nested rpc.batch() is not supported");
  }
  client_.batch_ = std::make_unique<BatchState>();
  active_batch = client_.batch_.get();
}

RpcClient::BatchGuard::~BatchGuard() {
  const bool should_send = std::uncaught_exceptions() == 0;
  if (should_send && active_batch == client_.batch_.get() && client_.batch_) {
    try {
      client_.batch_->send(client_);
    } catch (...) {
      // Destructors must not throw; batch errors surface only on explicit use.
    }
  }
  if (active_batch == client_.batch_.get()) {
    active_batch = nullptr;
  }
  client_.batch_.reset();
}

RpcClient::BatchGuard RpcClient::batch() { return BatchGuard(*this); }

RpcClient::RpcClient(std::string host, int port, double timeout_seconds)
    : host_(std::move(host)), port_(port), timeout_seconds_(timeout_seconds) {
  if (host_.empty() || port_ < 0) {
    const auto endpoint = airbus_endpoint();
    if (host_.empty()) host_ = endpoint.first;
    if (port_ < 0) port_ = endpoint.second;
  }
}

RpcClient::~RpcClient() = default;

RpcClient RpcClient::from_url(const std::string& url, double timeout_seconds) {
  const auto endpoint = parse_endpoint(url);
  return RpcClient(endpoint.first, endpoint.second, timeout_seconds);
}

nlohmann::json RpcClient::raw(const nlohmann::json& payload) {
  return raw_text(payload.dump());
}

nlohmann::json RpcClient::raw_text(const std::string& text) {
  const std::string body =
      detail::tcp_exchange(host_, port_, text, timeout_seconds_);
  if (body.empty()) {
    return nullptr;
  }
  return nlohmann::json::parse(body);
}

nlohmann::json RpcClient::call(const std::string& method,
                               const nlohmann::json& params,
                               const nlohmann::json& id) {
  if (active_batch != nullptr) {
    active_batch->enqueue_call(method, params, active_batch->next_id++);
    return nullptr;
  }
  return validate_response(raw(make_request(method, params, id)), id);
}

void RpcClient::notify(const std::string& method, const nlohmann::json& params) {
  if (active_batch != nullptr) {
    active_batch->enqueue_notify(method, params);
    return;
  }
  const auto response = raw(make_request(method, params, std::nullopt));
  if (!response.is_null()) {
    throw std::runtime_error("JSON-RPC notification must not produce a response");
  }
}

template <typename T>
T RpcClient::invoke(const std::string& method, const nlohmann::json& params) {
  nlohmann::json wire = params;
  if (!wire.is_null()) {
    validate_payload(method, "params", wire);
  } else {
    validate_payload(method, "params", nullptr);
  }
  if (active_batch != nullptr) {
    throw std::runtime_error(
        "typed RpcClient methods cannot run inside batch(); use call()/notify()");
  }
  constexpr int kId = 1;
  const auto response = call(method, wire, kId);
  return decode_result<T>(method, response_result(response));
}

PingResult RpcClient::ping() { return invoke<PingResult>("ping", nullptr); }

AddResult RpcClient::add(const AddParams& params) {
  return invoke<AddResult>("add", to_wire(params));
}

CreateQueueResult RpcClient::create_queue(const CreateQueueParams& params) {
  return invoke<CreateQueueResult>("create_queue", to_wire(params));
}

PostEventResult RpcClient::post_event(const PostEventParams& params) {
  return invoke<PostEventResult>("post_event", to_wire(params));
}

ListQueuesResult RpcClient::list_queues() {
  return invoke<ListQueuesResult>("list_queues", nullptr);
}

PeekEventsResult RpcClient::peek_events(const PeekEventsParams& params) {
  return invoke<PeekEventsResult>("peek_events", to_wire(params));
}

AttachListenerResult RpcClient::attach_listener(const AttachListenerParams& params) {
  return invoke<AttachListenerResult>("attach_listener", to_wire(params));
}

DetachListenerResult RpcClient::detach_listener(const DetachListenerParams& params) {
  return invoke<DetachListenerResult>("detach_listener", to_wire(params));
}

ListListenersResult RpcClient::list_listeners(
    const std::optional<ListListenersParams>& params) {
  nlohmann::json wire = nullptr;
  if (params.has_value()) {
    wire = to_wire(*params);
  }
  return invoke<ListListenersResult>("list_listeners", wire);
}

QueueReadyResult RpcClient::queue_ready(const QueueReadyParams& params) {
  return invoke<QueueReadyResult>("queue_ready", to_wire(params));
}

std::unique_ptr<EventListener> RpcClient::listen(
    const std::string& queue,
    std::function<void(const nlohmann::json&)> on_event, const std::string& host,
    std::optional<std::int64_t> exhaustion_timeout_ms,
    std::optional<std::int64_t> max_retries, std::optional<DuplexSide> side,
    std::optional<std::int64_t> max_events) {
  if (!on_event) {
    on_event = [](const nlohmann::json&) {};
  }
  return std::make_unique<EventListener>(*this, queue, std::move(on_event), host,
                                         exhaustion_timeout_ms, max_retries,
                                         max_events, side);
}

}  // namespace airbus
