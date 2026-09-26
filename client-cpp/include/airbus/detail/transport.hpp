#pragma once

#include <string>

namespace airbus {
namespace detail {

/** One-shot TCP exchange: connect, write payload, read until peer closes. */
std::string tcp_exchange(const std::string& host, int port,
                         const std::string& payload, double timeout_seconds);

}  // namespace detail
}  // namespace airbus
