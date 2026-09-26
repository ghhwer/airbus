#pragma once

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <functional>
#include <regex>
#include <stdexcept>
#include <string>
#include <thread>
#include <utility>

#include <signal.h>
#include <sys/select.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#include <airbus/rpc.hpp>

namespace airbus::test {

inline std::filesystem::path airbus_bin() {
  if (const char* env = std::getenv("AIRBUS_BIN"); env && env[0]) {
    return env;
  }
  // tests/ -> client-cpp/ -> repo root / out/airbus
  return std::filesystem::path(__FILE__).parent_path().parent_path().parent_path() /
         "out" / "airbus";
}

class AirbusServer {
 public:
  explicit AirbusServer(std::string bind = "127.0.0.1:0", double timeout = 5.0)
      : timeout_(timeout) {
    const auto bin = airbus_bin();
    if (!std::filesystem::exists(bin)) {
      throw std::runtime_error("AIRBUS_BIN not found: " + bin.string());
    }

    int stderr_pipe[2];
    if (::pipe(stderr_pipe) != 0) {
      throw std::runtime_error("pipe failed");
    }

    pid_ = ::fork();
    if (pid_ < 0) {
      throw std::runtime_error("fork failed");
    }
    if (pid_ == 0) {
      ::close(stderr_pipe[0]);
      ::dup2(stderr_pipe[1], STDERR_FILENO);
      ::close(stderr_pipe[1]);
      ::execl(bin.c_str(), bin.c_str(), "--listen", bind.c_str(),
              static_cast<char*>(nullptr));
      std::_Exit(127);
    }

    ::close(stderr_pipe[1]);
    stderr_fd_ = stderr_pipe[0];
    wait_for_listen();
  }

  ~AirbusServer() { stop(); }

  AirbusServer(const AirbusServer&) = delete;
  AirbusServer& operator=(const AirbusServer&) = delete;

  const std::string& host() const { return host_; }
  int port() const { return port_; }

  RpcClient client(double timeout = 2.0) const {
    return RpcClient(host_, port_, timeout);
  }

  void stop() {
    if (pid_ <= 0) return;
    ::kill(pid_, SIGTERM);
    int status = 0;
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::duration<double>(timeout_);
    while (std::chrono::steady_clock::now() < deadline) {
      const pid_t got = ::waitpid(pid_, &status, WNOHANG);
      if (got == pid_) {
        pid_ = -1;
        break;
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    if (pid_ > 0) {
      ::kill(pid_, SIGKILL);
      ::waitpid(pid_, &status, 0);
      pid_ = -1;
    }
    if (stderr_fd_ >= 0) {
      ::close(stderr_fd_);
      stderr_fd_ = -1;
    }
  }

 private:
  void wait_for_listen() {
    static const std::regex listen_re(R"(listening on ([\d.]+):(\d+))");
    std::string buf;
    char chunk[512];
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::duration<double>(timeout_);
    while (std::chrono::steady_clock::now() < deadline) {
      fd_set fds;
      FD_ZERO(&fds);
      FD_SET(stderr_fd_, &fds);
      timeval tv {};
      tv.tv_sec = 0;
      tv.tv_usec = 100000;
      const int ready = ::select(stderr_fd_ + 1, &fds, nullptr, nullptr, &tv);
      if (ready > 0) {
        const ssize_t n = ::read(stderr_fd_, chunk, sizeof(chunk));
        if (n > 0) {
          buf.append(chunk, static_cast<size_t>(n));
          std::smatch match;
          if (std::regex_search(buf, match, listen_re)) {
            host_ = match[1].str();
            port_ = std::stoi(match[2].str());
            return;
          }
        }
      }
      int status = 0;
      if (::waitpid(pid_, &status, WNOHANG) == pid_) {
        pid_ = -1;
        throw std::runtime_error("airbus exited before listen: " + buf);
      }
    }
    throw std::runtime_error("timed out waiting for airbus listen line: " + buf);
  }

  double timeout_;
  pid_t pid_ = -1;
  int stderr_fd_ = -1;
  std::string host_;
  int port_ = 0;
};

inline std::string unique_name(const std::string& prefix) {
  return prefix + "-" + std::to_string(
             std::chrono::steady_clock::now().time_since_epoch().count());
}

inline bool wait_until(std::chrono::milliseconds timeout,
                       const std::function<bool()>& pred) {
  const auto deadline = std::chrono::steady_clock::now() + timeout;
  while (std::chrono::steady_clock::now() < deadline) {
    if (pred()) return true;
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
  }
  return pred();
}

}  // namespace airbus::test
