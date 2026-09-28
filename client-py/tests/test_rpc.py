"""Integration tests against a live Airbus process over TCP."""

import re
import socket
import time
import uuid

import pytest

from airbus_client.payloads import (
    AttachListenerParams,
    CreateQueueParams,
    DeleteQueueParams,
    ListListenersParams,
    PeekEventsParams,
    PostEventParams,
    QueueMode,
)
from airbus_client.rpc import RpcClient, RpcError

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
        created = rpc.create_queue(CreateQueueParams(queue=queue))
        posted = rpc.post_event(PostEventParams(queue=queue, event=event))
        peeked = rpc.peek_events(PeekEventsParams(queue=queue))

    assert pong.result == "pong"
    assert created.result.queue == queue
    assert created.result.created is True
    assert posted.result.queue == queue
    assert UUID_V7.match(posted.result.id)
    assert peeked.result.queue == queue
    assert len(peeked.result.events) == 1
    assert peeked.result.events[0].event == event
    assert UUID_V7.match(peeked.result.events[0].id)


def test_get_events_method_not_found(rpc: RpcClient) -> None:
    response = rpc.call("get_events", params={"queue": "jobs"})
    assert response["error"]["code"] == -32601


def test_post_event_nonexistent_queue(rpc: RpcClient) -> None:
    queue = f"nonexistent-{uuid.uuid4()}"
    try:
        rpc.post_event(PostEventParams(queue=queue, event={"type": "fail"}))
        assert False, "expected RpcError"
    except RpcError as e:
        assert e.code == -32602
        assert "does not exist" in e.message


def test_post_event_invalid_params(rpc: RpcClient) -> None:
    response = rpc.call("post_event", params={})
    assert response["error"]["code"] == -32602


def test_list_and_peek_events(rpc: RpcClient) -> None:
    queue = f"jobs-{uuid.uuid4()}"
    other = f"other-{uuid.uuid4()}"
    rpc.create_queue(CreateQueueParams(queue=queue))
    rpc.create_queue(CreateQueueParams(queue=other))
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


def test_create_queue_modes(rpc: RpcClient) -> None:
    q1 = f"bcast-{uuid.uuid4()}"
    res1 = rpc.create_queue(CreateQueueParams(queue=q1, mode=QueueMode.broadcast))
    assert res1.queue == q1
    assert res1.mode == QueueMode.broadcast
    assert res1.created is True

    # Idempotent re-creation
    res1_again = rpc.create_queue(
        CreateQueueParams(queue=q1, mode=QueueMode.broadcast)
    )
    assert res1_again.created is False

    q2 = f"work-{uuid.uuid4()}"
    res2 = rpc.create_queue(CreateQueueParams(queue=q2, mode=QueueMode.fifo))
    assert res2.queue == q2
    assert res2.mode == QueueMode.fifo
    assert res2.created is True


def test_delete_queue(rpc: RpcClient) -> None:
    queue = f"delete-{uuid.uuid4()}"
    rpc.create_queue(CreateQueueParams(queue=queue))
    rpc.post_event(PostEventParams(queue=queue, event={"n": 1}))

    deleted = rpc.delete_queue(DeleteQueueParams(queue=queue))
    assert deleted.queue == queue
    assert deleted.deleted is True

    names = {q.name for q in rpc.list_queues().queues}
    assert queue not in names

    again = rpc.delete_queue(DeleteQueueParams(queue=queue))
    assert again.deleted is False

    with pytest.raises(RpcError) as exc:
        rpc.post_event(PostEventParams(queue=queue, event={"n": 2}))
    assert exc.value.code == -32602


def test_listen_broadcast(rpc: RpcClient) -> None:
    queue = f"bcast-{uuid.uuid4()}"
    rpc.create_queue(CreateQueueParams(queue=queue, mode=QueueMode.broadcast))

    events_a: list[dict] = []
    events_b: list[dict] = []

    with rpc.listen(queue, on_event=events_a.append), rpc.listen(
        queue, on_event=events_b.append
    ):
        listed = rpc.list_listeners(ListListenersParams(queue=queue))
        assert len(listed.listeners) == 2

        posted = rpc.post_event(
            PostEventParams(queue=queue, event={"broadcast": "hello"})
        )

        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline and (not events_a or not events_b):
            time.sleep(0.05)

        assert len(events_a) == 1
        assert len(events_b) == 1
        assert events_a[0]["id"] == posted.id
        assert events_a[0]["event"] == {"broadcast": "hello"}
        assert events_b[0]["id"] == posted.id
        assert events_b[0]["event"] == {"broadcast": "hello"}

    # Context exit auto-detaches
    listed_after = rpc.list_listeners(ListListenersParams(queue=queue))
    assert len(listed_after.listeners) == 0


def test_listen_competing_fifo(rpc: RpcClient) -> None:
    queue = f"fifo-{uuid.uuid4()}"
    rpc.create_queue(CreateQueueParams(queue=queue, mode=QueueMode.fifo))

    worker1_events: list[dict] = []
    worker2_events: list[dict] = []

    with rpc.listen(queue, on_event=worker1_events.append), rpc.listen(
        queue, on_event=worker2_events.append
    ):
        for i in range(4):
            rpc.post_event(PostEventParams(queue=queue, event={"task_id": i}))

        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline and (
            len(worker1_events) + len(worker2_events) < 4
        ):
            time.sleep(0.05)

        assert len(worker1_events) == 2
        assert len(worker2_events) == 2
        tasks1 = {e["event"]["task_id"] for e in worker1_events}
        tasks2 = {e["event"]["task_id"] for e in worker2_events}
        assert tasks1.isdisjoint(tasks2)
        assert tasks1 | tasks2 == {0, 1, 2, 3}


@pytest.mark.parametrize("exhaust", [False, True])
def test_broadcast_retries_only_unacknowledged_listeners(
    rpc: RpcClient, exhaust: bool
) -> None:
    queue = f"retry-{uuid.uuid4()}"
    rpc.create_queue(CreateQueueParams(queue=queue))
    healthy_events: list[dict] = []
    attempts: list[dict] = []

    def intermittent_listener(event: dict) -> None:
        attempts.append(event)
        if exhaust or len(attempts) == 1:
            raise RuntimeError("temporary delivery failure")

    with rpc.listen(queue, on_event=healthy_events.append), rpc.listen(
        queue, on_event=intermittent_listener, max_retries=2
    ):
        posted = rpc.post_event(PostEventParams(queue=queue, event={"retry": True}))
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            if len(attempts) == 2 and not rpc.peek_events(
                PeekEventsParams(queue=queue)
            ).events:
                break
            time.sleep(0.02)

        assert [event["id"] for event in healthy_events] == [posted.id]
        assert [event["id"] for event in attempts] == [posted.id, posted.id]
        assert not rpc.peek_events(PeekEventsParams(queue=queue)).events
        assert len(rpc.list_listeners(ListListenersParams(queue=queue)).listeners) == (
            1 if exhaust else 2
        )


def test_failed_listener_start_cleans_up(rpc: RpcClient) -> None:
    listener = rpc.listen(f"missing-{uuid.uuid4()}")
    with pytest.raises(RpcError, match="does not exist"), listener:
        pytest.fail("registration should fail")

    assert listener.listener_id is None
    assert listener._thread is None
    assert listener._server_sock.fileno() == -1


def test_server_listener_eviction_on_dead_port(rpc: RpcClient) -> None:
    queue = f"exhaust-{uuid.uuid4()}"
    rpc.create_queue(CreateQueueParams(queue=queue, mode=QueueMode.fifo))

    # Bind and close socket to obtain an unused port
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    dead_port = s.getsockname()[1]
    s.close()

    attached = rpc.attach_listener(
        AttachListenerParams(
            queue=queue,
            port=dead_port,
            host="127.0.0.1",
            max_retries=2,
            exhaustion_timeout_ms=100,
        )
    )
    assert attached.status == "attached"

    listeners = rpc.list_listeners(ListListenersParams(queue=queue))
    assert len(listeners.listeners) == 1

    # Publish an event to trigger delivery attempts to the dead port
    rpc.post_event(PostEventParams(queue=queue, event={"test": "dead"}))

    # Wait for exhaustion eviction
    deadline = time.monotonic() + 3.0
    evicted = False
    while time.monotonic() < deadline:
        time.sleep(0.1)
        listeners = rpc.list_listeners(ListListenersParams(queue=queue))
        if len(listeners.listeners) == 0:
            evicted = True
            break

    assert evicted, "Unreachable listener should have been evicted by Airbus"


def test_broadcast_retains_event_when_listener_dies_and_evicts(rpc: RpcClient) -> None:
    queue = f"retain-{uuid.uuid4()}"
    rpc.create_queue(CreateQueueParams(queue=queue, mode=QueueMode.broadcast))

    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    dead_port = s.getsockname()[1]
    s.close()

    rpc.attach_listener(
        AttachListenerParams(
            queue=queue,
            port=dead_port,
            host="127.0.0.1",
            max_retries=2,
            exhaustion_timeout_ms=100,
        )
    )

    posted = rpc.post_event(PostEventParams(queue=queue, event={"test": "survives"}))

    deadline = time.monotonic() + 3.0
    evicted = False
    while time.monotonic() < deadline:
        time.sleep(0.05)
        listeners = rpc.list_listeners(ListListenersParams(queue=queue))
        if len(listeners.listeners) == 0:
            evicted = True
            break

    assert evicted, "Unreachable listener should have been evicted"

    # Event must NOT be dropped!
    peek = rpc.peek_events(PeekEventsParams(queue=queue))
    assert len(peek.events) == 1
    assert peek.events[0].id == posted.id

    # A new listener attaching should receive the event
    received = []
    with rpc.listen(queue, on_event=received.append):
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            if len(received) == 1:
                break
            time.sleep(0.05)

    assert len(received) == 1
    assert received[0]["id"] == posted.id
    assert len(rpc.peek_events(PeekEventsParams(queue=queue)).events) == 0
