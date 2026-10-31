"""Named-queue handle integration tests."""

from __future__ import annotations

import threading
import time
import uuid

import pytest

from airbus_client import DispatchStrategy, DuplexSide, Queue, QueueMode, RpcClient
from airbus_client.payloads import DeleteQueueParams, QueueReadyParams
from airbus_client.protocol import RpcError


def test_full_duplex_cross_route(rpc: RpcClient) -> None:
    name = f"duplex-{uuid.uuid4()}"
    host_got: list[dict] = []
    device_got: list[dict] = []
    ready = threading.Event()

    host = Queue(name, mode=QueueMode.full_duplex, client=rpc, side=DuplexSide.host)
    # Second construct with same config is idempotent ensure
    device = Queue(name, mode=QueueMode.full_duplex, client=rpc, side=DuplexSide.device)

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


def test_ensure_rejects_mode_mismatch(rpc: RpcClient) -> None:
    name = f"mismatch-{uuid.uuid4()}"
    Queue(name, mode=QueueMode.fifo, client=rpc)
    with pytest.raises(RpcError, match="exists with mode="):
        Queue(name, mode=QueueMode.broadcast, client=rpc)


def test_ensure_rejects_strategy_mismatch(rpc: RpcClient) -> None:
    name = f"strat-{uuid.uuid4()}"
    Queue(
        name,
        mode=QueueMode.fifo,
        dispatch_strategy=DispatchStrategy.round_robin,
        client=rpc,
    )
    with pytest.raises(RpcError, match="strategy="):
        Queue(
            name,
            mode=QueueMode.fifo,
            dispatch_strategy=DispatchStrategy.single_node,
            client=rpc,
        )


def test_try_attach_backs_off_when_queue_deleted(rpc: RpcClient) -> None:
    name = f"missing-{uuid.uuid4()}"
    device = Queue(
        name, mode=QueueMode.full_duplex, client=rpc, side=DuplexSide.device
    )
    rpc.delete_queue(DeleteQueueParams(queue=name))
    assert device.try_attach(lambda _e: None) is None
    assert device.post({"n": 1}) is False
    assert device.is_ready() is False


def test_full_duplex_buffers_until_device_attaches(rpc: RpcClient) -> None:
    name = f"buffer-{uuid.uuid4()}"
    got: list[dict] = []
    ready = threading.Event()

    host = Queue(name, mode=QueueMode.full_duplex, client=rpc, side=DuplexSide.host)
    host.attach(lambda _e: None)
    assert host.is_ready() is False
    assert host.post({"buffered": True})

    device = Queue(name, mode=QueueMode.full_duplex, client=rpc, side=DuplexSide.device)
    device.attach(lambda e: (got.append(e), ready.set()))
    assert host.is_ready()
    assert ready.wait(timeout=2.0)
    assert got[-1]["event"]["buffered"] is True

    time.sleep(0.05)
    host.close()
    device.close()


def test_fifo_and_broadcast_ready_when_ensured(rpc: RpcClient) -> None:
    fifo_name = f"fifo-{uuid.uuid4()}"
    bcast_name = f"bcast-{uuid.uuid4()}"

    fifo = Queue(fifo_name, mode=QueueMode.fifo, client=rpc)
    assert fifo.is_ready() is True

    bcast = Queue(bcast_name, mode=QueueMode.broadcast, client=rpc)
    assert bcast.is_ready() is True

    assert rpc.queue_ready(QueueReadyParams(queue=fifo_name)).ready is True


def test_queue_delete(rpc: RpcClient) -> None:
    name = f"gone-{uuid.uuid4()}"
    q = Queue(name, mode=QueueMode.broadcast, client=rpc)
    assert q.delete() is True
    assert q.is_ready() is False
    assert q.delete() is False
    assert q.post({"n": 1}) is False
