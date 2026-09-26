#pragma once

#include <nlohmann/json.hpp>
#include <string>

namespace airbus {

/** Validate ``instance`` against a Draft 2020-12-ish schema subset used by Airbus payloads. */
void validate_schema(const nlohmann::json& schema, const nlohmann::json& instance,
                     const std::string& path = "");

}  // namespace airbus
