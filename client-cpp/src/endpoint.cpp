#include "airbus/endpoint.hpp"

#include <cstdlib>
#include <stdexcept>

namespace airbus {
namespace {

bool all_digits(const std::string& s) {
  if (s.empty()) return false;
  for (char c : s) {
    if (c < '0' || c > '9') return false;
  }
  return true;
}

}  // namespace

std::pair<std::string, int> parse_endpoint(const std::string& url) {
  const auto pos = url.rfind(':');
  if (pos == std::string::npos || pos == 0) {
    throw std::invalid_argument(
        "Airbus endpoint must be host:port (got '" + url +
        "'); example: 127.0.0.1:9097");
  }
  const std::string host = url.substr(0, pos);
  const std::string port_s = url.substr(pos + 1);
  if (host.empty() || !all_digits(port_s)) {
    throw std::invalid_argument(
        "Airbus endpoint must be host:port (got '" + url +
        "'); example: 127.0.0.1:9097");
  }
  return {host, std::stoi(port_s)};
}

std::pair<std::string, int> airbus_endpoint() {
  if (const char* url = std::getenv("AIRBUS_URL")) {
    const std::string trimmed = url;
    if (!trimmed.empty()) {
      return parse_endpoint(trimmed);
    }
  }
  const char* host_env = std::getenv("AIRBUS_HOST");
  const char* port_env = std::getenv("AIRBUS_PORT");
  const std::string host = host_env && host_env[0] ? host_env : kDefaultHost;
  const int port =
      port_env && port_env[0] ? std::stoi(port_env) : kDefaultPort;
  return {host, port};
}

std::string airbus_url() {
  const auto [host, port] = airbus_endpoint();
  return host + ":" + std::to_string(port);
}

}  // namespace airbus
