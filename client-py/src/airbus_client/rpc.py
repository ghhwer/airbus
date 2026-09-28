from __future__ import annotations

import json
import socket
import threading
from collections.abc import Callable
from contextlib import AbstractContextManager
from contextvars import ContextVar, Token
from functools import partial
from typing import Any, Generic, TypeVar

from airbus_client import protocol as proto
from airbus_client.payloads import (
    AddParams,
    AddResult,
    AttachListenerParams,
    AttachListenerResult,
    CreateQueueParams,
    CreateQueueResult,
    DetachListenerParams,
    DetachListenerResult,
    DuplexSide,
    ListListenersParams,
    ListListenersResult,
    ListQueuesResult,
    PeekEventsParams,
    PeekEventsResult,
    PingResult,
    PostEventParams,
    PostEventResult,
    QueueReadyParams,
    QueueReadyResult,
)
from airbus_client.protocol import RpcError

T = TypeVar("T")


class Pending(Generic[T]):
    """Placeholder filled when a batch is sent; read via `.result`."""

    def __init__(self) -> None:
        self._done = False
        self._value: T | None = None
        self._error: RpcError | None = None
        self._id: int | str | None = None

    @property
    def result(self) -> T:
        if not self._done:
            raise RuntimeError("batch has not been sent yet")
        if self._error is not None:
            raise self._error
        return self._value  # type: ignore[return-value]

    def _resolve(self, response: dict[str, Any], decode: Callable[[Any], T]) -> None:
        try:
            self._value = decode(proto.response_result(response))
        except RpcError as error:
            self._error = error
        self._done = True


class _BatchSession:
    """Internal queue for one JSON-RPC batch document."""

    def __init__(self, client: RpcClient) -> None:
        self._client = client
        self._next_id = 1
        self._requests: list[dict[str, Any]] = []
        self._slots: list[tuple[Pending[Any], Callable[[Any], Any]] | None] = []
        self._sent = False

    def enqueue_notify(self, method: str, params: Any = None) -> None:
        self._ensure_open()
        request = proto.request(method, params)
        self._requests.append(request)
        self._slots.append(None)

    def enqueue_call(
        self,
        method: str,
        params: Any,
        decode: Callable[[Any], T],
    ) -> Pending[T]:
        self._ensure_open()
        req_id = self._next_id
        self._next_id += 1
        request = proto.request(method, params, req_id)
        pending: Pending[T] = Pending()
        self._requests.append(request)
        self._slots.append((pending, decode))
        return pending

    def send(self) -> None:
        if self._sent:
            raise RuntimeError("batch already sent")
        if not self._requests:
            self._sent = True
            return

        response = self._client._raw(self._requests)
        responses = proto.batch_responses(self._requests, response)
        for document, slot in zip(responses, self._slots, strict=True):
            if slot is not None:
                pending, decode = slot
                assert document is not None
                pending._resolve(document, decode)

        self._sent = True

    def _ensure_open(self) -> None:
        if self._sent:
            raise RuntimeError("cannot enqueue on a sent batch")


_active_batch: ContextVar[_BatchSession | None] = ContextVar("airbus_rpc_batch", default=None)


class _BatchContext(AbstractContextManager["RpcClient"]):
    def __init__(self, client: RpcClient) -> None:
        self._client = client
        self._session: _BatchSession | None = None
        self._token: Token[_BatchSession | None] | None = None

    def __enter__(self) -> RpcClient:
        if _active_batch.get() is not None:
            raise RuntimeError("nested rpc.batch() is not supported")
        self._session = _BatchSession(self._client)
        self._token = _active_batch.set(self._session)
        return self._client

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        assert self._session is not None and self._token is not None
        try:
            if exc_type is None:
                self._session.send()
        finally:
            _active_batch.reset(self._token)
            self._session = None
            self._token = None


class EventListener:
    """Client-side port listener that receives real-time events pushed from Airbus."""

    def __init__(
        self,
        client: RpcClient,
        queue: str,
        on_event: Callable[[dict[str, Any]], Any],
        *,
        host: str = "127.0.0.1",
        exhaustion_timeout_ms: int | None = None,
        max_retries: int | None = None,
        max_events: int | None = None,
        side: DuplexSide | None = None,
    ) -> None:
        self._client = client
        self._queue = queue
        self._on_event = on_event
        self._host = host
        self._exhaustion_timeout_ms = exhaustion_timeout_ms
        self._max_retries = max_retries
        self._max_events = max_events
        self._side = side
    
        self._server_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._server_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._server_sock.bind((self._host, 0))
        self._server_sock.listen(128)
        self._port: int = self._server_sock.getsockname()[1]

        self._thread: threading.Thread | None = None
        self._running = False
        self._listener_id: str | None = None
        self.events: list[dict[str, Any]] = []

    @property
    def port(self) -> int:
        return self._port

    @property
    def listener_id(self) -> str | None:
        return self._listener_id

    def _serve(self) -> None:
        self._server_sock.settimeout(0.2)
        while self._running:
            try:
                conn, _ = self._server_sock.accept()
            except TimeoutError:
                continue
            except OSError:
                break

            try:
                chunks: list[bytes] = []
                while True:
                    chunk = conn.recv(4096)
                    if not chunk:
                        break
                    chunks.append(chunk)
                body = b"".join(chunks).decode().strip()
                if body:
                    response = proto.handle_event(body, self._receive_event)
                    if response is not None:
                        conn.sendall(json.dumps(response).encode())
                    try:
                        conn.shutdown(socket.SHUT_WR)
                    except OSError:
                        pass
            except Exception:
                pass
            finally:
                conn.close()

    def _receive_event(self, event: dict[str, Any]) -> None:
        if self._max_events is not None and len(self.events) >= self._max_events:
            self.events.pop(0)  # drop the oldest event
        # Add the new event to the end of the list
        self.events.append(event)
        self._on_event(event)

    def start(self) -> EventListener:
        self._running = True
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._thread.start()

        try:
            result = self._client.attach_listener(
                AttachListenerParams(
                    queue=self._queue,
                    port=self._port,
                    host=self._host,
                    exhaustion_timeout_ms=self._exhaustion_timeout_ms,
                    max_retries=self._max_retries,
                    side=self._side,
                )
            )
            assert isinstance(result, AttachListenerResult)
        except BaseException:
            self.close()
            raise
        self._listener_id = result.listener_id
        return self

    def close(self) -> None:
        if not self._running:
            return
        self._running = False
        if self._listener_id is not None:
            try:
                self._client.detach_listener(DetachListenerParams(listener_id=self._listener_id))
            except Exception:
                pass
            self._listener_id = None
        try:
            self._server_sock.close()
        except OSError:
            pass
        if self._thread is not None:
            self._thread.join(timeout=1.0)
            self._thread = None

    def __enter__(self) -> EventListener:
        return self.start()

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        self.close()


class RpcClient:
    def __init__(
        self,
        host: str | None = None,
        port: int | None = None,
        timeout: float = 2.0,
    ) -> None:
        if host is None or port is None:
            from airbus_client.endpoint import airbus_endpoint

            default_host, default_port = airbus_endpoint()
            host = default_host if host is None else host
            port = default_port if port is None else port
        self.host = host
        self.port = port
        self.timeout = timeout

    @classmethod
    def from_url(cls, url: str, *, timeout: float = 2.0) -> RpcClient:
        from airbus_client.endpoint import parse_endpoint

        host, port = parse_endpoint(url)
        return cls(host, port, timeout=timeout)

    def _raw(self, payload: Any) -> Any | None:
        return self._raw_text(json.dumps(payload))

    def _raw_text(self, text: str) -> Any | None:
        with socket.create_connection((self.host, self.port), timeout=self.timeout) as sock:
            sock.sendall(text.encode())
            sock.shutdown(socket.SHUT_WR)
            chunks: list[bytes] = []
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                chunks.append(chunk)
        body = b"".join(chunks).decode().strip()
        if not body:
            return None
        return json.loads(body)

    def batch(self) -> _BatchContext:
        """Defer subsequent calls/notifies into one JSON-RPC batch until the block exits."""
        return _BatchContext(self)

    def call(self, method: str, params: Any = None, id: int | str = 1) -> Any | Pending[Any]:
        batch = _active_batch.get()
        if batch is not None:
            return batch.enqueue_call(method, params, lambda result: result)

        return proto.validate_response(self._raw(proto.request(method, params, id)), id)

    def notify(self, method: str, params: Any = None) -> None:
        batch = _active_batch.get()
        if batch is not None:
            batch.enqueue_notify(method, params)
            return

        request = proto.request(method, params)
        if self._raw(request) is not None:
            raise AssertionError("JSON-RPC notification must not produce a response")

    def ping(self) -> PingResult | Pending[PingResult]:
        return self._invoke("ping", None, partial(proto.decode_result, "ping", PingResult))

    def add(self, params: AddParams) -> AddResult | Pending[AddResult]:
        return self._invoke("add", params, partial(proto.decode_result, "add", AddResult))

    def create_queue(
        self, params: CreateQueueParams
    ) -> CreateQueueResult | Pending[CreateQueueResult]:
        return self._invoke(
            "create_queue",
            params,
            partial(proto.decode_result, "create_queue", CreateQueueResult),
        )

    def post_event(self, params: PostEventParams) -> PostEventResult | Pending[PostEventResult]:
        return self._invoke(
            "post_event", params, partial(proto.decode_result, "post_event", PostEventResult)
        )

    def list_queues(self) -> ListQueuesResult | Pending[ListQueuesResult]:
        return self._invoke(
            "list_queues", None, partial(proto.decode_result, "list_queues", ListQueuesResult)
        )

    def peek_events(self, params: PeekEventsParams) -> PeekEventsResult | Pending[PeekEventsResult]:
        return self._invoke(
            "peek_events",
            params,
            partial(proto.decode_result, "peek_events", PeekEventsResult),
        )

    def attach_listener(
        self, params: AttachListenerParams
    ) -> AttachListenerResult | Pending[AttachListenerResult]:
        return self._invoke(
            "attach_listener",
            params,
            partial(proto.decode_result, "attach_listener", AttachListenerResult),
        )

    def detach_listener(
        self, params: DetachListenerParams
    ) -> DetachListenerResult | Pending[DetachListenerResult]:
        return self._invoke(
            "detach_listener",
            params,
            partial(proto.decode_result, "detach_listener", DetachListenerResult),
        )

    def list_listeners(
        self, params: ListListenersParams | None = None
    ) -> ListListenersResult | Pending[ListListenersResult]:
        return self._invoke(
            "list_listeners",
            params,
            partial(proto.decode_result, "list_listeners", ListListenersResult),
        )

    def queue_ready(
        self, params: QueueReadyParams
    ) -> QueueReadyResult | Pending[QueueReadyResult]:
        return self._invoke(
            "queue_ready",
            params,
            partial(proto.decode_result, "queue_ready", QueueReadyResult),
        )

    def listen(
        self,
        queue: str,
        on_event: Callable[[dict[str, Any]], Any] | None = None,
        *,
        host: str = "127.0.0.1",
        exhaustion_timeout_ms: int | None = None,
        max_retries: int | None = None,
        side: DuplexSide | None = None,
        max_events: int | None = None,
    ) -> EventListener:
        cb = on_event if on_event is not None else (lambda e: None)
        return EventListener(
            self,
            queue,
            cb,
            host=host,
            exhaustion_timeout_ms=exhaustion_timeout_ms,
            max_retries=max_retries,
            side=side,
            max_events=max_events,
        )

    def _invoke(
        self,
        method: str,
        params: Any,
        decode: Callable[[Any], T],
    ) -> T | Pending[T]:
        params = proto.to_wire(params)
        batch = _active_batch.get()
        if batch is not None:
            return batch.enqueue_call(method, params, decode)

        response = self.call(method, params=params)
        assert not isinstance(response, Pending)
        return decode(proto.response_result(response))
