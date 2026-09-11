"""Integration tests against a live Airbus process over TCP."""

import re
import uuid

from airbus_client.payloads import GetEventsParams, PeekEventsParams, PostEventParams
from airbus_client.rpc import RpcClient

UUID_V7 = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
)


def test_ping(rpc: RpcClient) -> None:
    response = rpc.call("ping")
    assert response["jsonrpc"] == "2.0"
    assert response["id"] == 1
    assert response["result"] == "pong"
    assert rpc.ping() == "pong"


def test_add(rpc: RpcClient) -> None:
    response = rpc.call("add", params=[2, 3])
    assert response["result"] == 5
    assert rpc.add([2, 3]) == 5.0


def test_notification_has_no_response(rpc: RpcClient) -> None:
    rpc.notify("ping")


def test_method_not_found(rpc: RpcClient) -> None:
    response = rpc.call("nope")
    assert response["error"]["code"] == -32601
    assert response["id"] == 1


def test_invalid_params(rpc: RpcClient) -> None:
    response = rpc.call("add", params=[1])
    assert response["error"]["code"] == -32602


def test_parse_error(rpc: RpcClient) -> None:
    response = rpc._raw_text("{")
    assert response["error"]["code"] == -32700
    assert response["id"] is None


def test_invalid_request(rpc: RpcClient) -> None:
    response = rpc._raw({"jsonrpc": "1.0", "method": "ping", "id": 1})
    assert response["error"]["code"] == -32600


def test_batch_ping_and_queue(rpc: RpcClient) -> None:
    queue = f"batch-{uuid.uuid4()}"
    event = {"type": "batch", "n": 1}
    with rpc.batch():
        pong = rpc.ping()
        rpc.notify("ping")
        posted = rpc.post_event(PostEventParams(queue=queue, event=event))
        got = rpc.get_events(GetEventsParams(queue=queue))

    assert pong.result == "pong"
    assert posted.result.queue == queue
    assert UUID_V7.match(posted.result.id)
    assert got.result.queue == queue
    assert got.result.events == [event]


def test_post_and_get_event(rpc: RpcClient) -> None:
    queue = f"jobs-{uuid.uuid4()}"
    event = {"type": "hello", "n": 1}
    posted = rpc.post_event(PostEventParams(queue=queue, event=event))
    assert posted.queue == queue
    assert UUID_V7.match(posted.id)

    got = rpc.get_events(GetEventsParams(queue=queue))
    assert got.queue == queue
    assert got.events == [event]


def test_get_events_empty_queue(rpc: RpcClient) -> None:
    queue = f"missing-{uuid.uuid4()}"
    got = rpc.get_events(GetEventsParams(queue=queue))
    assert got.queue == queue
    assert got.events == []


def test_get_events_default_count_is_one(rpc: RpcClient) -> None:
    queue = f"jobs-{uuid.uuid4()}"
    rpc.post_event(PostEventParams(queue=queue, event={"n": 1}))
    rpc.post_event(PostEventParams(queue=queue, event={"n": 2}))

    first = rpc.get_events(GetEventsParams(queue=queue))
    assert len(first.events) == 1

    second = rpc.get_events(GetEventsParams(queue=queue))
    assert len(second.events) == 1

    empty = rpc.get_events(GetEventsParams(queue=queue))
    assert empty.events == []


def test_get_events_respects_count(rpc: RpcClient) -> None:
    queue = f"jobs-{uuid.uuid4()}"
    for n in (1, 2, 3):
        rpc.post_event(PostEventParams(queue=queue, event={"n": n}))

    got = rpc.get_events(GetEventsParams(queue=queue, count=2))
    assert len(got.events) == 2

    rest = rpc.get_events(GetEventsParams(queue=queue, count=8))
    assert len(rest.events) == 1


def test_post_event_invalid_params(rpc: RpcClient) -> None:
    response = rpc.call("post_event", params={})
    assert response["error"]["code"] == -32602


def test_get_events_invalid_params(rpc: RpcClient) -> None:
    response = rpc.call("get_events", params={})
    assert response["error"]["code"] == -32602


def test_list_and_peek_events(rpc: RpcClient) -> None:
    queue = f"jobs-{uuid.uuid4()}"
    other = f"other-{uuid.uuid4()}"
    event = {"type": "hello", "n": 1}
    posted = rpc.post_event(PostEventParams(queue=queue, event=event))
    rpc.post_event(PostEventParams(queue=queue, event={"n": 2}))
    rpc.post_event(PostEventParams(queue=other, event={"x": True}))

    listed = rpc.list_queues()
    by_name = {item.name: item.depth for item in listed.queues}
    assert by_name[queue] == 2
    assert by_name[other] == 1

    peeked = rpc.peek_events(PeekEventsParams(queue=queue, count=10))
    assert peeked.queue == queue
    assert len(peeked.events) == 2
    assert {item.id for item in peeked.events} >= {posted.id}
    assert {item.event.get("n") for item in peeked.events} == {1, 2}
    assert next(item.event for item in peeked.events if item.id == posted.id) == event

    # peek does not consume
    again = rpc.peek_events(PeekEventsParams(queue=queue, count=10))
    assert len(again.events) == 2


def test_peek_events_empty_queue(rpc: RpcClient) -> None:
    queue = f"missing-{uuid.uuid4()}"
    peeked = rpc.peek_events(PeekEventsParams(queue=queue))
    assert peeked.queue == queue
    assert peeked.events == []


def test_peek_events_invalid_params(rpc: RpcClient) -> None:
    response = rpc.call("peek_events", params={})
    assert response["error"]["code"] == -32602
