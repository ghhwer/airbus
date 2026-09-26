#pragma once

#include <functional>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "airbus/error.hpp"
#include "airbus/payloads.hpp"

namespace airbus {

inline constexpr const char* kJsonRpcVersion = "2.0";

/** Convert payload structs / JSON to wire values (omit nullopt fields via to_json). */
nlohmann::json to_wire(const nlohmann::json& value);
template <typename T>
nlohmann::json to_wire(const T& value) {
  return nlohmann::json(value);
}

void validate_payload(const std::string& method, const std::string& kind,
                      const nlohmann::json& value);

template <typename T>
T decode_result(const std::string& method, const nlohmann::json& value) {
  validate_payload(method, "result", value);
  return value.get<T>();
}

nlohmann::json make_request(const std::string& method,
                            const nlohmann::json& params = nullptr,
                            const std::optional<nlohmann::json>& id = nlohmann::json(1));

nlohmann::json validate_response(const nlohmann::json& response,
                                 const nlohmann::json& expected_id);

nlohmann::json response_result(const nlohmann::json& response);

std::vector<std::optional<nlohmann::json>> batch_responses(
    const std::vector<nlohmann::json>& requests, const nlohmann::json& response);

/** Handle an inbound ``on_event`` body; returns response object or nullopt for notifications. */
std::optional<nlohmann::json> handle_event(
    const std::string& body,
    const std::function<void(const nlohmann::json&)>& callback);

}  // namespace airbus
