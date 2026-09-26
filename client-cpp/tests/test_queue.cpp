#include <atomic>
#include <chrono>
#include <vector>

#include <catch2/catch_test_macros.hpp>

#include <airbus/client.hpp>

#include "helpers/airbus_server.hpp"

using airbus::test::AirbusServer;
using airbus::test::unique_name;
using airbus::test::wait_until;

TEST_CASE("full duplex cross route", "[queue][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  const std::string name = unique_name("duplex");

  std::vector<nlohmann::json> host_got;
  std::vector<nlohmann::json> device_got;
  std::atomic<bool> ready{false};

  airbus::Queue host(name, &rpc, airbus::DuplexSide::Host);
  airbus::Queue device(name, &rpc, airbus::DuplexSide::Device);

  REQUIRE(host.create(airbus::QueueMode::FullDuplex));
  REQUIRE_FALSE(host.create(airbus::QueueMode::FullDuplex));

  host.attach([&](const nlohmann::json& e) { host_got.push_back(e); });
  REQUIRE(device.try_attach([&](const nlohmann::json& e) {
    device_got.push_back(e);
    ready = true;
  }) != nullptr);
  REQUIRE(host.is_ready());

  REQUIRE(host.post({{"from", "host"}, {"n", 1}}));
  REQUIRE(wait_until(std::chrono::seconds(2), [&] { return ready.load(); }));
  REQUIRE(device_got.back()["event"]["from"] == "host");

  ready = false;
  device_got.clear();
  host.close();
  host.attach([&](const nlohmann::json& e) {
    host_got.push_back(e);
    ready = true;
  });
  REQUIRE(device.post({{"from", "device"}, {"n", 2}}));
  REQUIRE(wait_until(std::chrono::seconds(2), [&] { return ready.load(); }));
  REQUIRE(host_got.back()["event"]["from"] == "device");

  host.close();
  device.close();
}

TEST_CASE("device try_attach backs off when missing", "[queue][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  airbus::Queue device(unique_name("missing"), &rpc, airbus::DuplexSide::Device);
  REQUIRE(device.try_attach([](const nlohmann::json&) {}) == nullptr);
  REQUIRE_FALSE(device.post({{"n", 1}}));
  REQUIRE_FALSE(device.is_ready());
}

TEST_CASE("full duplex buffers until device attaches", "[queue][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  const std::string name = unique_name("buffer");

  std::vector<nlohmann::json> got;
  std::atomic<bool> ready{false};

  airbus::Queue host(name, &rpc, airbus::DuplexSide::Host);
  host.create(airbus::QueueMode::FullDuplex);
  host.attach([](const nlohmann::json&) {});
  REQUIRE_FALSE(host.is_ready());
  REQUIRE(host.post({{"buffered", true}}));

  airbus::Queue device(name, &rpc, airbus::DuplexSide::Device);
  device.attach([&](const nlohmann::json& e) {
    got.push_back(e);
    ready = true;
  });
  REQUIRE(host.is_ready());
  REQUIRE(wait_until(std::chrono::seconds(2), [&] { return ready.load(); }));
  REQUIRE(got.back()["event"]["buffered"] == true);

  host.close();
  device.close();
}

TEST_CASE("fifo and broadcast ready when queue exists", "[queue][integration]") {
  AirbusServer server;
  auto rpc = server.client();

  airbus::Queue fifo(unique_name("fifo"), &rpc);
  REQUIRE_FALSE(fifo.is_ready());
  REQUIRE(fifo.create(airbus::QueueMode::Fifo));
  REQUIRE(fifo.is_ready());

  airbus::Queue bcast(unique_name("bcast"), &rpc);
  REQUIRE(bcast.create(airbus::QueueMode::Broadcast));
  REQUIRE(bcast.is_ready());

  airbus::QueueReadyParams params;
  params.queue = fifo.name();
  REQUIRE(rpc.queue_ready(params).ready);
}
