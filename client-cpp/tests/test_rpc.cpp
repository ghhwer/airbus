#include <chrono>
#include <map>
#include <set>
#include <string>

#include <catch2/catch_test_macros.hpp>

#include <airbus/client.hpp>

#include "helpers/airbus_server.hpp"

using airbus::test::AirbusServer;
using airbus::test::unique_name;
using airbus::test::wait_until;

TEST_CASE("ping and add", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();

  auto response = rpc.call("ping");
  REQUIRE(response["jsonrpc"] == "2.0");
  REQUIRE(response["id"] == 1);
  REQUIRE(response["result"] == "pong");
  REQUIRE(rpc.ping() == "pong");

  response = rpc.call("add", nlohmann::json::array({2, 3}));
  REQUIRE(response["result"] == 5);
  REQUIRE(rpc.add({2.0, 3.0}) == 5.0);
}

TEST_CASE("notification has no response", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  REQUIRE_NOTHROW(rpc.notify("ping"));
}

TEST_CASE("method not found and invalid params", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();

  auto response = rpc.call("nope");
  REQUIRE(response["error"]["code"] == -32601);

  response = rpc.call("add", nlohmann::json::array({1}));
  REQUIRE(response["error"]["code"] == -32602);
}

TEST_CASE("parse error and invalid request", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();

  auto response = rpc.raw_text("{");
  REQUIRE(response["error"]["code"] == -32700);
  REQUIRE(response["id"].is_null());

  response = rpc.raw({{"jsonrpc", "1.0"}, {"method", "ping"}, {"id", 1}});
  REQUIRE(response["error"]["code"] == -32600);
}

TEST_CASE("batch call and notify", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  const std::string queue = unique_name("batch");

  {
    auto guard = rpc.batch();
    rpc.call("ping");
    rpc.notify("ping");
    rpc.call("create_queue", {{"queue", queue}, {"mode", "fifo"}});
  }

  auto listed = rpc.list_queues();
  bool found = false;
  for (const auto& item : listed.queues) {
    if (item.name == queue) found = true;
  }
  REQUIRE(found);
}

TEST_CASE("post peek list queues", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  const std::string queue = unique_name("jobs");
  const std::string other = unique_name("other");

  airbus::CreateQueueParams create;
  create.queue = queue;
  rpc.create_queue(create);
  create.queue = other;
  rpc.create_queue(create);

  airbus::PostEventParams post;
  post.queue = queue;
  post.event = {{"type", "hello"}, {"n", 1}};
  auto posted = rpc.post_event(post);
  REQUIRE(posted.queue == queue);
  REQUIRE_FALSE(posted.id.empty());

  post.event = {{"n", 2}};
  rpc.post_event(post);
  post.queue = other;
  post.event = {{"x", true}};
  rpc.post_event(post);

  auto listed = rpc.list_queues();
  std::map<std::string, std::int64_t> by_name;
  for (const auto& item : listed.queues) {
    by_name[item.name] = item.depth;
  }
  REQUIRE(by_name[queue] == 2);
  REQUIRE(by_name[other] == 1);

  airbus::PeekEventsParams peek;
  peek.queue = queue;
  peek.count = 10;
  auto peeked = rpc.peek_events(peek);
  REQUIRE(peeked.queue == queue);
  REQUIRE(peeked.events.size() == 2);
  REQUIRE(rpc.peek_events(peek).events.size() == 2);  // non-destructive
}

TEST_CASE("post nonexistent queue raises", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  airbus::PostEventParams post;
  post.queue = unique_name("missing");
  post.event = {{"type", "fail"}};
  try {
    rpc.post_event(post);
    FAIL("expected RpcError");
  } catch (const airbus::RpcError& error) {
    REQUIRE(error.code() == -32602);
    REQUIRE(error.message().find("does not exist") != std::string::npos);
  }
}

TEST_CASE("create queue modes", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  const std::string q1 = unique_name("bcast");
  airbus::CreateQueueParams params;
  params.queue = q1;
  params.mode = airbus::QueueMode::Broadcast;
  auto res = rpc.create_queue(params);
  REQUIRE(res.created);
  REQUIRE(res.mode == airbus::QueueMode::Broadcast);
  REQUIRE_FALSE(rpc.create_queue(params).created);

  params.queue = unique_name("fifo");
  params.mode = airbus::QueueMode::Fifo;
  res = rpc.create_queue(params);
  REQUIRE(res.created);
  REQUIRE(res.mode == airbus::QueueMode::Fifo);
}

TEST_CASE("listen broadcast", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  const std::string queue = unique_name("bcast");

  airbus::CreateQueueParams create;
  create.queue = queue;
  create.mode = airbus::QueueMode::Broadcast;
  rpc.create_queue(create);

  std::vector<nlohmann::json> events_a;
  std::vector<nlohmann::json> events_b;
  auto listener_a = rpc.listen(queue, [&](const nlohmann::json& e) { events_a.push_back(e); });
  auto listener_b = rpc.listen(queue, [&](const nlohmann::json& e) { events_b.push_back(e); });
  listener_a->start();
  listener_b->start();

  airbus::ListListenersParams list_params;
  list_params.queue = queue;
  REQUIRE(rpc.list_listeners(list_params).listeners.size() == 2);

  airbus::PostEventParams post;
  post.queue = queue;
  post.event = {{"broadcast", "hello"}};
  auto posted = rpc.post_event(post);

  REQUIRE(wait_until(std::chrono::seconds(3), [&] {
    return !events_a.empty() && !events_b.empty();
  }));
  REQUIRE(events_a.size() == 1);
  REQUIRE(events_b.size() == 1);
  REQUIRE(events_a[0]["id"] == posted.id);
  REQUIRE(events_b[0]["id"] == posted.id);

  listener_a->close();
  listener_b->close();
  REQUIRE(rpc.list_listeners(list_params).listeners.empty());
}

TEST_CASE("listen competing fifo", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  const std::string queue = unique_name("fifo");

  airbus::CreateQueueParams create;
  create.queue = queue;
  create.mode = airbus::QueueMode::Fifo;
  rpc.create_queue(create);

  std::vector<nlohmann::json> worker1;
  std::vector<nlohmann::json> worker2;
  auto l1 = rpc.listen(queue, [&](const nlohmann::json& e) { worker1.push_back(e); });
  auto l2 = rpc.listen(queue, [&](const nlohmann::json& e) { worker2.push_back(e); });
  l1->start();
  l2->start();

  for (int i = 0; i < 4; ++i) {
    airbus::PostEventParams post;
    post.queue = queue;
    post.event = {{"task_id", i}};
    rpc.post_event(post);
  }

  REQUIRE(wait_until(std::chrono::seconds(3), [&] {
    return worker1.size() + worker2.size() >= 4;
  }));
  REQUIRE(worker1.size() == 2);
  REQUIRE(worker2.size() == 2);

  std::set<int> tasks1;
  std::set<int> tasks2;
  for (const auto& e : worker1) tasks1.insert(e["event"]["task_id"].get<int>());
  for (const auto& e : worker2) tasks2.insert(e["event"]["task_id"].get<int>());
  for (int t : tasks1) {
    REQUIRE(tasks2.count(t) == 0);
  }

  l1->close();
  l2->close();
}

TEST_CASE("failed listener start cleans up", "[rpc][integration]") {
  AirbusServer server;
  auto rpc = server.client();
  auto listener = rpc.listen(unique_name("missing"));
  try {
    listener->start();
    FAIL("expected RpcError");
  } catch (const airbus::RpcError& error) {
    REQUIRE(error.message().find("does not exist") != std::string::npos);
  }
  REQUIRE_FALSE(listener->listener_id().has_value());
}
