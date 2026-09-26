#include "airbus/rpc.hpp"

#include <arpa/inet.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>

#include <chrono>
#include <cstring>
#include <stdexcept>
#include <thread>

#include "airbus/protocol.hpp"

namespace airbus {

EventListener::EventListener(RpcClient& client, std::string queue,
                             std::function<void(const nlohmann::json&)> on_event,
                             std::string host,
                             std::optional<std::int64_t> exhaustion_timeout_ms,
                             std::optional<std::int64_t> max_retries,
                             std::optional<std::int64_t> max_events,
                             std::optional<DuplexSide> side)
    : client_(&client),
      queue_(std::move(queue)),
      on_event_(std::move(on_event)),
      host_(std::move(host)),
      exhaustion_timeout_ms_(exhaustion_timeout_ms),
      max_retries_(max_retries),
      max_events_(max_events),
      side_(side) {
  server_fd_ = ::socket(AF_INET, SOCK_STREAM, 0);
  if (server_fd_ < 0) {
    throw std::runtime_error("failed to create listener socket");
  }
  int yes = 1;
  setsockopt(server_fd_, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes));

  sockaddr_in addr {};
  addr.sin_family = AF_INET;
  addr.sin_port = htons(0);
  if (::inet_pton(AF_INET, host_.c_str(), &addr.sin_addr) != 1) {
    ::close(server_fd_);
    server_fd_ = -1;
    throw std::runtime_error("invalid listen host " + host_);
  }
  if (::bind(server_fd_, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) != 0) {
    ::close(server_fd_);
    server_fd_ = -1;
    throw std::runtime_error("failed to bind listener socket");
  }
  if (::listen(server_fd_, 128) != 0) {
    ::close(server_fd_);
    server_fd_ = -1;
    throw std::runtime_error("failed to listen on listener socket");
  }
  socklen_t len = sizeof(addr);
  if (::getsockname(server_fd_, reinterpret_cast<sockaddr*>(&addr), &len) != 0) {
    ::close(server_fd_);
    server_fd_ = -1;
    throw std::runtime_error("failed to read listener port");
  }
  port_ = ntohs(addr.sin_port);
}

EventListener::~EventListener() { close(); }

void EventListener::serve() {
  while (running_) {
    fd_set fds;
    FD_ZERO(&fds);
    FD_SET(server_fd_, &fds);
    timeval tv {};
    tv.tv_sec = 0;
    tv.tv_usec = 200000;
    const int ready = ::select(server_fd_ + 1, &fds, nullptr, nullptr, &tv);
    if (ready < 0) {
      break;
    }
    if (ready == 0) {
      continue;
    }
    const int conn = ::accept(server_fd_, nullptr, nullptr);
    if (conn < 0) {
      continue;
    }
    try {
      std::string body;
      char buffer[4096];
      while (true) {
        const ssize_t n = ::recv(conn, buffer, sizeof(buffer), 0);
        if (n < 0) break;
        if (n == 0) break;
        body.append(buffer, static_cast<size_t>(n));
      }
      while (!body.empty() &&
             (body.back() == '\n' || body.back() == '\r' || body.back() == ' ')) {
        body.pop_back();
      }
      if (!body.empty()) {
        const auto response = handle_event(body, [this](const nlohmann::json& event) {
          receive_event(event);
        });
        if (response.has_value()) {
          const std::string out = response->dump();
          ::send(conn, out.data(), out.size(), 0);
        }
        ::shutdown(conn, SHUT_WR);
      }
    } catch (...) {
      // Match Python: swallow per-connection errors.
    }
    ::close(conn);
  }
}

void EventListener::receive_event(const nlohmann::json& event) {
  if (max_events_.has_value() &&
      static_cast<std::int64_t>(events_.size()) >= *max_events_ && !events_.empty()) {
    events_.erase(events_.begin());
  }
  events_.push_back(event);
  on_event_(event);
}

EventListener& EventListener::start() {
  running_ = true;
  thread_ = std::make_unique<std::thread>([this] { serve(); });
  try {
    AttachListenerParams params;
    params.queue = queue_;
    params.port = port_;
    params.host = host_;
    params.exhaustion_timeout_ms = exhaustion_timeout_ms_;
    params.max_retries = max_retries_;
    params.side = side_;
    const auto result = client_->attach_listener(params);
    listener_id_ = result.listener_id;
  } catch (...) {
    close();
    throw;
  }
  return *this;
}

void EventListener::close() {
  if (!running_ && server_fd_ < 0 && !thread_) {
    return;
  }
  running_ = false;
  if (listener_id_.has_value()) {
    try {
      DetachListenerParams params;
      params.listener_id = *listener_id_;
      client_->detach_listener(params);
    } catch (...) {
    }
    listener_id_.reset();
  }
  if (server_fd_ >= 0) {
    ::close(server_fd_);
    server_fd_ = -1;
  }
  if (thread_ && thread_->joinable()) {
    thread_->join();
  }
  thread_.reset();
}

}  // namespace airbus
