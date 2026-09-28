"""Named-queue handle integration tests."""

from __future__ import annotations

import threading
import time
import uuid

from airbus_client import DuplexSide, Queue, QueueMode, RpcClient
from airbus_client.payloads import CreateQueueParams, QueueReadyParams


def test_full_duplex_cross_route(rpc: RpcClient) -> None:
    name = f"duplex-{uuid.uuid4()}"
    host_got: list[dict] = []
    device_got: list[dict] = []
    ready = threading.Event()

    host = Queue(name, client=rpc, side=DuplexSide.host)
    device = Queue(name, client=rpc, side=DuplexSide.device)

    assert host.create(QueueMode.full_duplex) is True
    assert host.create(QueueMode.full_duplex) is False  # idempotent

    host.attach(lambda e: host_got.append(e))
    assert device.try_attach(lambda e: (device_got.append(e), ready.set())) is not None
    assert host.is_ready()

    assert host.post({"from": "host", "n": 1})
    assert ready.wait(timeout=2.0)
    assert device_got[-1]["event"]["from"] == "host"

    ready.clear()
    device_got.clear()

    def on_host(e: dict) -> None:
        host_got.append(e)
        ready.set()

    host.close()
    host.attach(on_host)
    assert device.post({"from": "device", "n": 2})
    assert ready.wait(timeout=2.0)
    assert host_got[-1]["event"]["from"] == "device"

    host.close()
    device.close()


def test_device_try_attach_backs_off_when_missing(rpc: RpcClient) -> None:
    name = f"missing-{uuid.uuid4()}"
    device = Queue(name, client=rpc, side=DuplexSide.device)
    assert device.try_attach(lambda _e: None) is None
    assert device.post({"n": 1}) is False
    assert device.is_ready() is False


def test_full_duplex_buffers_until_device_attaches(rpc: RpcClient) -> None:
    name = f"buffer-{uuid.uuid4()}"
    got: list[dict] = []
    ready = threading.Event()

    host = Queue(name, client=rpc, side=DuplexSide.host)
    host.create(QueueMode.full_duplex)
    host.attach(lambda _e: None)
    assert host.is_ready() is False
    assert host.post({"buffered": True})

    device = Queue(name, client=rpc, side=DuplexSide.device)
    device.attach(lambda e: (got.append(e), ready.set()))
    assert host.is_ready()
    assert ready.wait(timeout=2.0)
    assert got[-1]["event"]["buffered"] is True

    time.sleep(0.05)
    host.close()
    device.close()


def test_fifo_and_broadcast_ready_when_queue_exists(rpc: RpcClient) -> None:
    fifo_name = f"fifo-{uuid.uuid4()}"
    bcast_name = f"bcast-{uuid.uuid4()}"

    fifo = Queue(fifo_name, client=rpc)
    assert fifo.is_ready() is False
    assert fifo.create(QueueMode.fifo) is True
    assert fifo.is_ready() is True

    bcast = Queue(bcast_name, client=rpc)
    assert bcast.create(QueueMode.broadcast) is True
    assert bcast.is_ready() is True

    # Engine RPC matches handle
    assert rpc.queue_ready(QueueReadyParams(queue=fifo_name)).ready is True
    rpc.create_queue(CreateQueueParams(queue=f"raw-{uuid.uuid4()}", mode=QueueMode.fifo))


def test_queue_delete(rpc: RpcClient) -> None:
    name = f"gone-{uuid.uuid4()}"
    q = Queue(name, client=rpc)
    assert q.create(QueueMode.broadcast) is True
    assert q.delete() is True
    assert q.is_ready() is False
    assert q.delete() is False
    assert q.post({"n": 1}) is False
