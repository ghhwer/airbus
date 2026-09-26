#include <stdexcept>

#include <catch2/catch_test_macros.hpp>

#include <airbus/client.hpp>
#include <airbus/validate.hpp>

TEST_CASE("make_request builds envelope", "[protocol]") {
  const auto req = airbus::make_request("ping", nullptr, 1);
  REQUIRE(req["jsonrpc"] == "2.0");
  REQUIRE(req["method"] == "ping");
  REQUIRE(req["id"] == 1);
  REQUIRE_FALSE(req.contains("params"));
}

TEST_CASE("validate_payload rejects bad add params", "[protocol]") {
  REQUIRE_THROWS_AS(airbus::validate_payload("add", "params", nlohmann::json::array({1})),
                    std::invalid_argument);
}

TEST_CASE("validate_payload accepts add params", "[protocol]") {
  REQUIRE_NOTHROW(
      airbus::validate_payload("add", "params", nlohmann::json::array({2.0, 3.0})));
}

TEST_CASE("decode_result ping", "[protocol]") {
  const auto value = airbus::decode_result<airbus::PingResult>("ping", "pong");
  REQUIRE(value == "pong");
}

TEST_CASE("handle_event acknowledges on_event", "[protocol]") {
  nlohmann::json seen;
  const auto body = nlohmann::json{
      {"jsonrpc", "2.0"},
      {"id", 7},
      {"method", "on_event"},
      {"params",
       {{"queue", "q"}, {"id", "evt-1"}, {"event", {{"n", 1}}}}},
  }.dump();
  const auto response = airbus::handle_event(body, [&](const nlohmann::json& event) {
    seen = event;
  });
  REQUIRE(response.has_value());
  REQUIRE((*response)["id"] == 7);
  REQUIRE((*response)["result"]["status"] == "ok");
  REQUIRE(seen["queue"] == "q");
  REQUIRE(seen["id"] == "evt-1");
  REQUIRE(seen["event"]["n"] == 1);
}

TEST_CASE("payload round-trip CreateQueueParams", "[protocol]") {
  airbus::CreateQueueParams params;
  params.queue = "jobs";
  params.mode = airbus::QueueMode::Fifo;
  const nlohmann::json wire = params;
  auto back = wire.get<airbus::CreateQueueParams>();
  REQUIRE(back.queue == "jobs");
  REQUIRE(back.mode == airbus::QueueMode::Fifo);
}
