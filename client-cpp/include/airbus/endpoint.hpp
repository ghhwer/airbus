#pragma once

#include <string>
#include <utility>

namespace airbus {

inline constexpr const char* kDefaultHost = "127.0.0.1";
inline constexpr int kDefaultPort = 9097;

/** Parse ``host:port`` into a host/port pair. */
std::pair<std::string, int> parse_endpoint(const std::string& url);

/**
 * Resolve host/port from ``AIRBUS_URL``, else ``AIRBUS_HOST`` / ``AIRBUS_PORT``,
 * else ``127.0.0.1:9097``.
 */
std::pair<std::string, int> airbus_endpoint();

/** Return ``host:port`` for the resolved endpoint. */
std::string airbus_url();

}  // namespace airbus
