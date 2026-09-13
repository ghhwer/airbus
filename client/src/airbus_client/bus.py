"""Thin full-duplex channel helper — create/bind/send over Airbus ``full-duplex`` queues.

Routing and listener integrity live in the Rust engine; this module only wraps
create / attach / post / presence checks.
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

from airbus_client.payloads import (
    CreateQueueParams,
    DuplexSide,
    ListListenersParams,
    PostEventParams,
    QueueMode,
)
from airbus_client.protocol import RpcError
from airbus_client.rpc import EventListener, RpcClient


class BusChannel:
    """One side of a full-duplex channel (`host` or `device`)."""

    def __init__(
        self,
        client: RpcClient,
        name: str,
        side: DuplexSide,
        *,
        host: str = "127.0.0.1",
    ) -> None:
        self._client = client
        self.name = name
        self.side = side
        self._listen_host = host
        self._listener: EventListener | None = None

    def create(self) -> bool:
        """Create the full-duplex channel (host responsibility). Returns whether newly created."""
        result = self._client.create_queue(
            CreateQueueParams(queue=self.name, mode=QueueMode.full_duplex)
        )
        assert not isinstance(result, type(None))
        return bool(result.created)

    def bind(self, on_event: Callable[[dict[str, Any]], Any]) -> EventListener:
        """Attach this side's listener. Raises RpcError if the channel does not exist."""
        if self._listener is not None:
            self.close()
        listener = self._client.listen(
            self.name,
            on_event,
            host=self._listen_host,
            side=self.side,
            max_events=100, # TODO: We need a better handle on this, keeping hardcoded for now, but the binding process is a bit complicated we can improve this later.
        )
        listener.start()
        self._listener = listener
        return listener

    def try_bind(self, on_event: Callable[[dict[str, Any]], Any]) -> EventListener | None:
        """Bind if the channel exists; return None when missing (device backoff path)."""
        try:
            return self.bind(on_event)
        except RpcError as exc:
            if exc.code == -32602 and "does not exist" in str(exc):
                return None
            raise

    def send(self, event: dict[str, Any]) -> bool:
        """Post an event from this side. Returns False if the channel is missing."""
        try:
            self._client.post_event(
                PostEventParams(queue=self.name, event=event, side=self.side)
            )
            return True
        except RpcError as exc:
            if exc.code == -32602 and "does not exist" in str(exc):
                return False
            raise

    def attached(self) -> bool:
        """True when both host and device listeners are present."""
        try:
            listed = self._client.list_listeners(ListListenersParams(queue=self.name))
        except RpcError:
            return False
        sides = {item.side for item in listed.listeners if item.side is not None}
        return DuplexSide.host in sides and DuplexSide.device in sides

    def close(self) -> None:
        if self._listener is not None:
            self._listener.close()
            self._listener = None

    def __enter__(self) -> BusChannel:
        return self

    def __exit__(self, *args: object) -> None:
        self.close()
