"""Public airbus_client API.

Import from this package root for application use. Wire ``*Params`` / ``*Result``
types live under ``airbus_client.payloads`` for raw RPC. ``protocol``, ``contracts``,
``binary``, and ``endpoint`` are operational internals.

Endpoint defaults: ``AIRBUS_URL`` or ``AIRBUS_HOST``/``AIRBUS_PORT`` (else
``127.0.0.1:9097``). ``RpcClient()`` and ``Queue(name)`` use that resolution;
pass ``client=`` for ephemeral test servers.
"""

from airbus_client.binary import run_airbus
from airbus_client.payloads import (
    DispatchStrategy,
    DuplexSide,
    ListenerEventParams,
    QueueMode,
)
from airbus_client.queue import Queue
from airbus_client.rpc import EventListener, RpcClient, RpcError
from airbus_client.server import start_server

__all__ = [
    "DispatchStrategy",
    "DuplexSide",
    "EventListener",
    "ListenerEventParams",
    "Queue",
    "QueueMode",
    "RpcClient",
    "RpcError",
    "run_airbus",
    "start_server",
]
