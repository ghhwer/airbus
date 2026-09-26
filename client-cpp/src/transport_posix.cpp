#include "airbus/detail/transport.hpp"

#include <arpa/inet.h>
#include <netdb.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>

#include <stdexcept>
#include <string>

namespace airbus {
namespace detail {

std::string tcp_exchange(const std::string& host, int port,
                         const std::string& payload, double timeout_seconds) {
  addrinfo hints {};
  hints.ai_family = AF_UNSPEC;
  hints.ai_socktype = SOCK_STREAM;

  addrinfo* result = nullptr;
  const std::string port_s = std::to_string(port);
  const int rc = getaddrinfo(host.c_str(), port_s.c_str(), &hints, &result);
  if (rc != 0) {
    throw std::runtime_error(std::string("getaddrinfo: ") + gai_strerror(rc));
  }

  int fd = -1;
  for (addrinfo* rp = result; rp != nullptr; rp = rp->ai_next) {
    fd = ::socket(rp->ai_family, rp->ai_socktype, rp->ai_protocol);
    if (fd < 0) continue;

    timeval tv {};
    tv.tv_sec = static_cast<int>(timeout_seconds);
    tv.tv_usec = static_cast<int>((timeout_seconds - tv.tv_sec) * 1e6);
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));

    if (::connect(fd, rp->ai_addr, rp->ai_addrlen) == 0) {
      break;
    }
    ::close(fd);
    fd = -1;
  }
  freeaddrinfo(result);

  if (fd < 0) {
    throw std::runtime_error("failed to connect to " + host + ":" +
                             std::to_string(port));
  }

  const char* data = payload.data();
  size_t remaining = payload.size();
  while (remaining > 0) {
    const ssize_t n = ::send(fd, data, remaining, 0);
    if (n < 0) {
      ::close(fd);
      throw std::runtime_error("send failed");
    }
    data += n;
    remaining -= static_cast<size_t>(n);
  }
  ::shutdown(fd, SHUT_WR);

  std::string body;
  char buffer[4096];
  while (true) {
    const ssize_t n = ::recv(fd, buffer, sizeof(buffer), 0);
    if (n < 0) {
      ::close(fd);
      throw std::runtime_error("recv failed");
    }
    if (n == 0) break;
    body.append(buffer, static_cast<size_t>(n));
  }
  ::close(fd);

  while (!body.empty() &&
         (body.back() == '\n' || body.back() == '\r' || body.back() == ' ')) {
    body.pop_back();
  }
  return body;
}

}  // namespace detail
}  // namespace airbus
