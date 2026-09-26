"""Unit tests for Airbus endpoint env resolution (no live server)."""

from __future__ import annotations

import pytest

from airbus_client import RpcClient
from airbus_client.endpoint import airbus_endpoint, airbus_url, parse_endpoint


def test_airbus_endpoint_defaults(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("AIRBUS_URL", raising=False)
    monkeypatch.delenv("AIRBUS_HOST", raising=False)
    monkeypatch.delenv("AIRBUS_PORT", raising=False)
    assert airbus_endpoint() == ("127.0.0.1", 9097)
    assert airbus_url() == "127.0.0.1:9097"


def test_airbus_endpoint_from_url(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("AIRBUS_URL", "10.0.0.2:19097")
    assert airbus_endpoint() == ("10.0.0.2", 19097)


def test_airbus_endpoint_from_host_port(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("AIRBUS_URL", raising=False)
    monkeypatch.setenv("AIRBUS_HOST", "localhost")
    monkeypatch.setenv("AIRBUS_PORT", "9098")
    assert airbus_endpoint() == ("localhost", 9098)


def test_parse_endpoint_rejects_bad_url() -> None:
    with pytest.raises(ValueError, match="host:port"):
        parse_endpoint("not-a-host-port")


def test_rpc_client_uses_env_defaults(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("AIRBUS_URL", "127.0.0.1:9099")
    client = RpcClient(timeout=1.5)
    assert client.host == "127.0.0.1"
    assert client.port == 9099
    assert client.timeout == 1.5


def test_rpc_client_from_url() -> None:
    client = RpcClient.from_url("10.1.2.3:4040", timeout=3.0)
    assert client.host == "10.1.2.3"
    assert client.port == 4040
    assert client.timeout == 3.0
