#pragma once

#include <stdexcept>
#include <string>

namespace airbus {

class RpcError : public std::runtime_error {
 public:
  RpcError(int code, std::string message, std::string data = {},
           std::string id = {})
      : std::runtime_error(std::to_string(code) + ": " + message),
        code_(code),
        message_(std::move(message)),
        data_(std::move(data)),
        id_(std::move(id)) {}

  int code() const { return code_; }
  const std::string& message() const { return message_; }
  const std::string& data() const { return data_; }
  const std::string& id() const { return id_; }

 private:
  int code_;
  std::string message_;
  std::string data_;
  std::string id_;
};

}  // namespace airbus
