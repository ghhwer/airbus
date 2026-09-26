#include "airbus/protocol.hpp"

#include <map>
#include <stdexcept>
#include <utility>

#include "airbus/contracts.hpp"
#include "airbus/validate.hpp"

namespace airbus {

nlohmann::json to_wire(const nlohmann::json& value) { return value; }

void validate_payload(const std::string& method, const std::string& kind,
                      const nlohmann::json& value) {
  const auto& methods = contracts::methods();
  const auto it = methods.find(method);
  if (it == methods.end()) {
    throw std::invalid_argument("unknown method " + method);
  }
  const auto& info = it->second;
  if (kind == "params") {
    if (value.is_null()) {
      if (info.params_required) {
        throw std::invalid_argument(method + " params: required");
      }
      if (info.params.empty()) {
        return;
      }
      // optional params omitted
      return;
    }
    if (info.params.empty()) {
      if (!(value.is_null() || (value.is_object() && value.empty()))) {
        throw std::invalid_argument(method + " takes no parameters");
      }
      return;
    }
    validate_schema(contracts::schemas().at(info.params), value);
    return;
  }
  if (kind == "result") {
    validate_schema(contracts::schemas().at(info.result), value);
    return;
  }
  throw std::invalid_argument("unknown payload kind " + kind);
}

nlohmann::json make_request(const std::string& method, const nlohmann::json& params,
                            const std::optional<nlohmann::json>& id) {
  nlohmann::json document = {{"jsonrpc", kJsonRpcVersion}, {"method", method}};
  if (id.has_value()) {
    document["id"] = *id;
  }
  if (!params.is_null()) {
    document["params"] = params;
  }
  return document;
}

namespace {

bool valid_id(const nlohmann::json& value) {
  return value.is_null() || value.is_number_integer() || value.is_string();
}

}  // namespace

nlohmann::json validate_response(const nlohmann::json& response,
                                 const nlohmann::json& expected_id) {
  if (!response.is_object() || response.value("jsonrpc", "") != kJsonRpcVersion) {
    throw std::invalid_argument("invalid JSON-RPC response version");
  }
  if (!response.contains("id") || !valid_id(response["id"])) {
    throw std::invalid_argument("invalid JSON-RPC response id");
  }
  if (response["id"] != expected_id) {
    throw std::invalid_argument("JSON-RPC response id does not match request");
  }
  const bool has_result = response.contains("result");
  const bool has_error = response.contains("error");
  if (has_result == has_error) {
    throw std::invalid_argument(
        "response must contain exactly one of result or error");
  }
  if (has_error) {
    const auto& error = response["error"];
    if (!error.is_object() || !error.contains("code") ||
        !error["code"].is_number_integer() || !error.contains("message") ||
        !error["message"].is_string()) {
      throw std::invalid_argument("invalid JSON-RPC error object");
    }
  }
  return response;
}

nlohmann::json response_result(const nlohmann::json& response) {
  if (response.contains("error")) {
    const auto& error = response["error"];
    std::string data;
    if (error.contains("data")) {
      data = error["data"].dump();
    }
    throw RpcError(error["code"].get<int>(), error["message"].get<std::string>(),
                   std::move(data), response["id"].dump());
  }
  return response["result"];
}

std::vector<std::optional<nlohmann::json>> batch_responses(
    const std::vector<nlohmann::json>& requests, const nlohmann::json& response) {
  std::vector<nlohmann::json> expected_ids;
  for (const auto& item : requests) {
    if (item.contains("id")) {
      expected_ids.push_back(item["id"]);
    }
  }
  if (expected_ids.empty()) {
    if (!response.is_null()) {
      throw std::invalid_argument("notifications must not receive a response");
    }
    return std::vector<std::optional<nlohmann::json>>(requests.size(), std::nullopt);
  }
  if (!response.is_array()) {
    throw std::invalid_argument("batch response must be an array");
  }
  std::map<nlohmann::json, nlohmann::json> by_id;
  for (const auto& item : response) {
    if (!item.is_object() || !item.contains("id") || !valid_id(item["id"])) {
      throw std::invalid_argument("invalid batch response");
    }
    const auto& id = item["id"];
    bool found = false;
    for (const auto& expected : expected_ids) {
      if (expected == id) {
        found = true;
        break;
      }
    }
    if (!found || by_id.find(id) != by_id.end()) {
      throw std::invalid_argument("unexpected or duplicate batch response id");
    }
    by_id.emplace(id, validate_response(item, id));
  }
  if (by_id.size() != expected_ids.size()) {
    throw std::invalid_argument("missing batch response");
  }
  std::vector<std::optional<nlohmann::json>> out;
  out.reserve(requests.size());
  for (const auto& item : requests) {
    if (item.contains("id")) {
      out.push_back(by_id.at(item["id"]));
    } else {
      out.push_back(std::nullopt);
    }
  }
  return out;
}

std::optional<nlohmann::json> handle_event(
    const std::string& body,
    const std::function<void(const nlohmann::json&)>& callback) {
  nlohmann::json document;
  try {
    document = nlohmann::json::parse(body);
  } catch (const nlohmann::json::parse_error&) {
    return nlohmann::json{
        {"jsonrpc", kJsonRpcVersion},
        {"id", nullptr},
        {"error", {{"code", -32700}, {"message", "invalid JSON"}}},
    };
  }
  if (!document.is_object()) {
    return nlohmann::json{
        {"jsonrpc", kJsonRpcVersion},
        {"id", nullptr},
        {"error", {{"code", -32600}, {"message", "invalid request"}}},
    };
  }
  const nlohmann::json id =
      document.contains("id") ? document["id"] : nlohmann::json(nullptr);
  const bool notification = !document.contains("id");
  if (document.value("jsonrpc", "") != kJsonRpcVersion || !valid_id(id) ||
      !document.contains("method") || !document["method"].is_string()) {
    return nlohmann::json{
        {"jsonrpc", kJsonRpcVersion},
        {"id", valid_id(id) ? id : nullptr},
        {"error", {{"code", -32600}, {"message", "invalid request"}}},
    };
  }
  if (document["method"].get<std::string>() != "on_event") {
    if (notification) return std::nullopt;
    return nlohmann::json{
        {"jsonrpc", kJsonRpcVersion},
        {"id", id},
        {"error", {{"code", -32601}, {"message", "method not found"}}},
    };
  }
  try {
    const nlohmann::json params =
        document.contains("params") ? document["params"] : nlohmann::json(nullptr);
    validate_payload("on_event", "params", params);
    const auto event = params.get<ListenerEventParams>();
    callback(to_wire(event));
  } catch (const std::invalid_argument& error) {
    if (notification) return std::nullopt;
    return nlohmann::json{
        {"jsonrpc", kJsonRpcVersion},
        {"id", id},
        {"error", {{"code", -32602}, {"message", error.what()}}},
    };
  } catch (const std::exception& error) {
    if (notification) return std::nullopt;
    return nlohmann::json{
        {"jsonrpc", kJsonRpcVersion},
        {"id", id},
        {"error", {{"code", -32603}, {"message", error.what()}}},
    };
  }
  if (notification) return std::nullopt;
  ListenerEventResult acknowledgment;
  acknowledgment.status = Status::Ok;
  const nlohmann::json result = to_wire(acknowledgment);
  validate_payload("on_event", "result", result);
  return nlohmann::json{
      {"jsonrpc", kJsonRpcVersion},
      {"id", id},
      {"result", result},
  };
}

}  // namespace airbus
