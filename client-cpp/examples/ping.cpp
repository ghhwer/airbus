#include <cstdlib>
#include <iostream>

#include <airbus/client.hpp>

int main() {
  try {
    airbus::RpcClient client;
    const airbus::PingResult pong = client.ping();
    std::cout << pong << "\n";
    return pong == "pong" ? 0 : 1;
  } catch (const std::exception& ex) {
    std::cerr << "airbus ping failed: " << ex.what() << "\n";
    return 1;
  }
}
