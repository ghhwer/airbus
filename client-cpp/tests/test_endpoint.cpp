#include <cstdlib>
#include <stdexcept>

#include <catch2/catch_test_macros.hpp>

#include <airbus/client.hpp>

TEST_CASE("endpoint defaults", "[endpoint]") {
  unsetenv("AIRBUS_URL");
  unsetenv("AIRBUS_HOST");
  unsetenv("AIRBUS_PORT");
  const auto endpoint = airbus::airbus_endpoint();
  REQUIRE(endpoint.first == "127.0.0.1");
  REQUIRE(endpoint.second == 9097);
  REQUIRE(airbus::airbus_url() == "127.0.0.1:9097");
}

TEST_CASE("endpoint from AIRBUS_URL", "[endpoint]") {
  setenv("AIRBUS_URL", "10.0.0.2:19097", 1);
  const auto endpoint = airbus::airbus_endpoint();
  REQUIRE(endpoint.first == "10.0.0.2");
  REQUIRE(endpoint.second == 19097);
  unsetenv("AIRBUS_URL");
}

TEST_CASE("endpoint from host/port", "[endpoint]") {
  unsetenv("AIRBUS_URL");
  setenv("AIRBUS_HOST", "localhost", 1);
  setenv("AIRBUS_PORT", "9098", 1);
  const auto endpoint = airbus::airbus_endpoint();
  REQUIRE(endpoint.first == "localhost");
  REQUIRE(endpoint.second == 9098);
  unsetenv("AIRBUS_HOST");
  unsetenv("AIRBUS_PORT");
}

TEST_CASE("parse_endpoint rejects bad url", "[endpoint]") {
  REQUIRE_THROWS_AS(airbus::parse_endpoint("not-a-host-port"), std::invalid_argument);
}

TEST_CASE("RpcClient uses env defaults", "[endpoint]") {
  setenv("AIRBUS_URL", "127.0.0.1:9099", 1);
  airbus::RpcClient client({}, -1, 1.5);
  REQUIRE(client.host() == "127.0.0.1");
  REQUIRE(client.port() == 9099);
  unsetenv("AIRBUS_URL");
}

TEST_CASE("RpcClient::from_url", "[endpoint]") {
  auto client = airbus::RpcClient::from_url("10.1.2.3:4040", 3.0);
  REQUIRE(client.host() == "10.1.2.3");
  REQUIRE(client.port() == 4040);
}
