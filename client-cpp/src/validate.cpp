#include "airbus/validate.hpp"

#include <stdexcept>
#include <string>

namespace airbus {
namespace {

[[noreturn]] void fail(const std::string& path, const std::string& message) {
  throw std::invalid_argument(path.empty() ? message : path + ": " + message);
}

const nlohmann::json* resolve_ref(const nlohmann::json& root, const nlohmann::json& node) {
  if (!node.contains("$ref")) {
    return &node;
  }
  const auto& ref = node.at("$ref").get_ref<const std::string&>();
  const std::string prefix = "#/$defs/";
  if (ref.size() < prefix.size() || ref.compare(0, prefix.size(), prefix) != 0) {
    fail("", "unsupported $ref " + ref);
  }
  const std::string key = ref.substr(prefix.size());
  if (!root.contains("$defs") || !root.at("$defs").contains(key)) {
    fail("", "unknown $ref " + ref);
  }
  return &root.at("$defs").at(key);
}

void validate_impl(const nlohmann::json& root, const nlohmann::json& schema,
                   const nlohmann::json& instance, const std::string& path);

void validate_type(const std::string& type, const nlohmann::json& instance,
                   const std::string& path) {
  if (type == "object") {
    if (!instance.is_object()) fail(path, "expected object");
  } else if (type == "array") {
    if (!instance.is_array()) fail(path, "expected array");
  } else if (type == "string") {
    if (!instance.is_string()) fail(path, "expected string");
  } else if (type == "integer") {
    if (!instance.is_number_integer()) fail(path, "expected integer");
  } else if (type == "number") {
    if (!instance.is_number()) fail(path, "expected number");
  } else if (type == "boolean") {
    if (!instance.is_boolean()) fail(path, "expected boolean");
  } else if (type == "null") {
    if (!instance.is_null()) fail(path, "expected null");
  }
}

void validate_impl(const nlohmann::json& root, const nlohmann::json& schema_in,
                   const nlohmann::json& instance, const std::string& path) {
  const nlohmann::json* schema_ptr = resolve_ref(root, schema_in);
  const nlohmann::json& schema = *schema_ptr;

  if (schema.contains("type")) {
    if (schema.at("type").is_string()) {
      validate_type(schema.at("type").get<std::string>(), instance, path);
    }
  }

  if (schema.contains("enum")) {
    bool ok = false;
    for (const auto& item : schema.at("enum")) {
      if (item == instance) {
        ok = true;
        break;
      }
    }
    if (!ok) fail(path, "value not in enum");
  }

  if (schema.contains("minLength") && instance.is_string()) {
    if (static_cast<int>(instance.get_ref<const std::string&>().size()) <
        schema.at("minLength").get<int>()) {
      fail(path, "string shorter than minLength");
    }
  }

  if (instance.is_number_integer() || instance.is_number_unsigned()) {
    const auto number = instance.get<std::int64_t>();
    if (schema.contains("minimum") && number < schema.at("minimum").get<std::int64_t>()) {
      fail(path, "below minimum");
    }
    if (schema.contains("maximum") && number > schema.at("maximum").get<std::int64_t>()) {
      fail(path, "above maximum");
    }
  }

  if (instance.is_array()) {
    if (schema.contains("minItems") &&
        static_cast<int>(instance.size()) < schema.at("minItems").get<int>()) {
      fail(path, "array shorter than minItems");
    }
    if (schema.contains("maxItems") &&
        static_cast<int>(instance.size()) > schema.at("maxItems").get<int>()) {
      fail(path, "array longer than maxItems");
    }
    if (schema.contains("items")) {
      const auto& items = schema.at("items");
      for (std::size_t i = 0; i < instance.size(); ++i) {
        validate_impl(root, items, instance.at(i), path + "[" + std::to_string(i) + "]");
      }
    }
  }

  if (instance.is_object() && schema.contains("properties")) {
    const auto& properties = schema.at("properties");
    if (schema.value("additionalProperties", true) == false) {
      for (auto it = instance.begin(); it != instance.end(); ++it) {
        if (!properties.contains(it.key())) {
          fail(path, "additional property '" + it.key() + "'");
        }
      }
    }
    if (schema.contains("required")) {
      for (const auto& req : schema.at("required")) {
        const auto key = req.get<std::string>();
        if (!instance.contains(key)) {
          fail(path, "missing required property '" + key + "'");
        }
      }
    }
    for (auto it = properties.begin(); it != properties.end(); ++it) {
      if (!instance.contains(it.key())) continue;
      const std::string child = path.empty() ? it.key() : path + "." + it.key();
      validate_impl(root, it.value(), instance.at(it.key()), child);
    }
  }
}

}  // namespace

void validate_schema(const nlohmann::json& schema, const nlohmann::json& instance,
                     const std::string& path) {
  validate_impl(schema, schema, instance, path);
}

}  // namespace airbus
