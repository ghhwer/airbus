"""Thin named-queue handle — create / attach / post / readiness over Airbus RPC.

Mode and side policy live in the Rust engine; this module only wraps RPC and owns
the local listener port via ``EventListener``.

When ``client`` is omitted, uses ``RpcClient()`` (``AIRBUS_URL`` / host+port env,
default ``127.0.0.1:9097``).
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

from airbus_client.payloads import (
    CreateQueueParams,
    DispatchStrategy,
    DuplexSide,
    PostEventParams,
    QueueMode,
    QueueReadyParams,
)
from airbus_client.protocol import RpcError
from airbus_client.rpc import EventListener, RpcClient


class Queue:
    """Named queue handle for any ``QueueMode`` (optional sticky duplex ``side``)."""

    def __init__(
        self,
        name: str,
        *,
        client: RpcClient | None = None,
        side: DuplexSide | None = None,
        listen_host: str = "127.0.0.1",
    ) -> None:
        self._client = client if client is not None else RpcClient()
        self.name = name
        self.side = side
        self._listen_host = listen_host
        self._listener: EventListener | None = None

    def create(
        self,
        mode: QueueMode,
        *,
        dispatch_strategy: DispatchStrategy | None = None,
    ) -> bool:
        """Create the queue. Returns whether it was newly created."""
        result = self._client.create_queue(
            CreateQueueParams(
                queue=self.name,
                mode=mode,
                dispatch_strategy=dispatch_strategy,
            )
        )
        assert not isinstance(result, type(None))
        return bool(result.created)

    def attach(
        self,
        on_event: Callable[[dict[str, Any]], Any],
        *,
        max_events: int | None = 100,
        exhaustion_timeout_ms: int | None = None,
        max_retries: int | None = None,
    ) -> EventListener:
        """Attach a listener. Raises RpcError if the queue does not exist or params are invalid."""
        if self._listener is not None:
            self.close()
        listener = self._client.listen(
            self.name,
            on_event,
            host=self._listen_host,
            side=self.side,
            max_events=max_events,
            exhaustion_timeout_ms=exhaustion_timeout_ms,
            max_retries=max_retries,
        )
        listener.start()
        self._listener = listener
        return listener

    def try_attach(
        self,
        on_event: Callable[[dict[str, Any]], Any],
        **kwargs: Any,
    ) -> EventListener | None:
        """Attach if the queue exists; return None when missing (attachable backoff path)."""
        try:
            return self.attach(on_event, **kwargs)
        except RpcError as exc:
            if exc.code == -32602 and "does not exist" in str(exc):
                return None
            raise

    def post(self, event: dict[str, Any]) -> bool:
        """Post an event. Returns False if the queue is missing."""
        try:
            self._client.post_event(
                PostEventParams(queue=self.name, event=event, side=self.side)
            )
            return True
        except RpcError as exc:
            if exc.code == -32602 and "does not exist" in str(exc):
                return False
            raise

    def is_ready(self) -> bool:
        """Ask Airbus whether this queue is ready for its mode."""
        result = self._client.queue_ready(QueueReadyParams(queue=self.name))
        assert not isinstance(result, type(None))
        return bool(result.ready)

    def close(self) -> None:
        if self._listener is not None:
            self._listener.close()
            self._listener = None

    def __enter__(self) -> Queue:
        return self

    def __exit__(self, *args: object) -> None:
        self.close()
