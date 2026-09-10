"""Integration tests: spawn the Airbus binary and speak JSON-RPC over TCP."""

from collections.abc import Iterator

import pytest

from airbus_client.rpc import RpcClient
from airbus_client.server import start_server


@pytest.fixture(scope="module")
def rpc() -> Iterator[RpcClient]:
    with start_server() as client:
        yield client
