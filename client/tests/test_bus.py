"""Full-duplex channel integration tests."""

from __future__ import annotations

import threading
import time
import uuid

from airbus_client import BusChannel, DuplexSide, RpcClient


def test_full_duplex_cross_route(rpc: RpcClient) -> None:
    name = f"duplex-{uuid.uuid4()}"
    host_got: list[dict] = []
    device_got: list[dict] = []
    ready = threading.Event()

    host = BusChannel(rpc, name, DuplexSide.host)
    device = BusChannel(rpc, name, DuplexSide.device)

    assert host.create() is True
    assert host.create() is False  # idempotent

    host.bind(lambda e: host_got.append(e))
    assert device.try_bind(lambda e: (device_got.append(e), ready.set())) is not None
    assert host.attached()

    assert host.send({"from": "host", "n": 1})
    assert ready.wait(timeout=2.0)
    assert device_got[-1]["event"]["from"] == "host"

    ready.clear()
    device_got.clear()

    def on_host(e: dict) -> None:
        host_got.append(e)
        ready.set()

    host.close()
    host.bind(on_host)
    assert device.send({"from": "device", "n": 2})
    assert ready.wait(timeout=2.0)
    assert host_got[-1]["event"]["from"] == "device"

    host.close()
    device.close()


def test_device_try_bind_backs_off_when_missing(rpc: RpcClient) -> None:
    name = f"missing-{uuid.uuid4()}"
    device = BusChannel(rpc, name, DuplexSide.device)
    assert device.try_bind(lambda _e: None) is None
    assert device.send({"n": 1}) is False


def test_full_duplex_buffers_until_device_binds(rpc: RpcClient) -> None:
    name = f"buffer-{uuid.uuid4()}"
    got: list[dict] = []
    ready = threading.Event()

    host = BusChannel(rpc, name, DuplexSide.host)
    host.create()
    host.bind(lambda _e: None)
    assert host.send({"buffered": True})

    device = BusChannel(rpc, name, DuplexSide.device)
    device.bind(lambda e: (got.append(e), ready.set()))
    assert ready.wait(timeout=2.0)
    assert got[-1]["event"]["buffered"] is True

    time.sleep(0.05)
    host.close()
    device.close()
