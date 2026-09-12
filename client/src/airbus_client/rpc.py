from __future__ import annotations

import json
import socket
import threading
from collections.abc import Callable
from contextlib import AbstractContextManager
from contextvars import ContextVar, Token
from dataclasses import asdict, is_dataclass
from typing import Any, Generic, TypeVar

from airbus_client.payloads import (
    AddParams,
    AddResult,
    AttachListenerParams,
    AttachListenerResult,
    CreateQueueParams,
    CreateQueueResult,
    DetachListenerParams,
    DetachListenerResult,
    Event,
    GetEventsParams,
    GetEventsResult,
    Listener,
    ListListenersParams,
    ListListenersResult,
    ListQueuesResult,
    PeekEventsParams,
    PeekEventsResult,
    PingResult,
    PostEventParams,
    PostEventResult,
    Queue,
    QueueMode,
)

T = TypeVar("T")


def _to_jsonable(value: Any) -> Any:
    if is_dataclass(value) and not isinstance(value, type):
        return asdict(value)
    return value


def _from_post_event_result(data: dict[str, Any]) -> PostEventResult:
    return PostEventResult(id=data["id"], queue=data["queue"])


def _from_get_events_result(data: dict[str, Any]) -> GetEventsResult:
    return GetEventsResult(queue=data["queue"], events=list(data["events"]))


def _from_list_queues_result(data: dict[str, Any]) -> ListQueuesResult:
    queues = [
        Queue(
            name=item["name"],
            depth=int(item["depth"]),
            mode=QueueMode(item["mode"]),
            listener_count=int(item["listener_count"]),
        )
        for item in data["queues"]
    ]
    return ListQueuesResult(queues=queues)


def _from_peek_events_result(data: dict[str, Any]) -> PeekEventsResult:
    events = [
        Event(id=item["id"], event=dict(item["event"])) for item in data["events"]
    ]
    return PeekEventsResult(queue=data["queue"], events=events)


def _from_create_queue_result(data: dict[str, Any]) -> CreateQueueResult:
    return CreateQueueResult(
        queue=data["queue"],
        mode=QueueMode(data["mode"]),
        created=bool(data["created"]),
    )


def _from_attach_listener_result(data: dict[str, Any]) -> AttachListenerResult:
    return AttachListenerResult(
        listener_id=data["listener_id"],
        queue=data["queue"],
        status=data["status"],
    )


def _from_detach_listener_result(data: dict[str, Any]) -> DetachListenerResult:
    return DetachListenerResult(
        listener_id=data["listener_id"],
        detached=bool(data["detached"]),
    )


def _from_list_listeners_result(data: dict[str, Any]) -> ListListenersResult:
    listeners = [
        Listener(
            id=item["id"],
            queue=item["queue"],
            host=item["host"],
            port=int(item["port"]),
            mode=QueueMode(item["mode"]),
            failure_count=int(item["failure_count"]),
            active=bool(item["active"]),
        )
        for item in data["listeners"]
    ]
    return ListListenersResult(listeners=listeners)


def _get_events_wire_params(params: GetEventsParams) -> dict[str, Any]:
    payload: dict[str, Any] = {"queue": params.queue}
    if params.count is not None:
        payload["count"] = params.count
    return payload


def _peek_events_wire_params(params: PeekEventsParams) -> dict[str, Any]:
    payload: dict[str, Any] = {"queue": params.queue}
    if params.count is not None:
        payload["count"] = params.count
    return payload


def _create_queue_wire_params(params: CreateQueueParams) -> dict[str, Any]:
    payload: dict[str, Any] = {"queue": params.queue}
    if params.mode is not None:
        payload["mode"] = str(params.mode)
    if params.dispatch_strategy is not None:
        payload["dispatch_strategy"] = str(params.dispatch_strategy)
    return payload


def _attach_listener_wire_params(params: AttachListenerParams) -> dict[str, Any]:
    payload: dict[str, Any] = {"queue": params.queue, "port": params.port}
    if params.host is not None:
        payload["host"] = params.host
    if params.exhaustion_timeout_ms is not None:
        payload["exhaustion_timeout_ms"] = params.exhaustion_timeout_ms
    if params.max_retries is not None:
        payload["max_retries"] = params.max_retries
    return payload


def _detach_listener_wire_params(params: DetachListenerParams) -> dict[str, Any]:
    return {"listener_id": params.listener_id}


def _list_listeners_wire_params(
    params: ListListenersParams | None,
) -> dict[str, Any] | None:
    if params is None:
        return None
    payload: dict[str, Any] = {}
    if params.queue is not None:
        payload["queue"] = params.queue
    return payload


def _decode_ping(result: Any) -> PingResult:
    assert isinstance(result, str)
    return result


def _decode_add(result: Any) -> AddResult:
    return float(result)


class RpcError(Exception):
    """JSON-RPC error object from a call or batch slot."""

    def __init__(
        self,
        code: int,
        message: str,
        data: Any = None,
        *,
        id: int | str | None = None,
    ) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message
        self.data = data
        self.id = id


class Pending(Generic[T]):
    """Placeholder filled when a batch is sent; read via `.result`."""

    def __init__(self) -> None:
        self._done = False
        self._value: T | None = None
        self._error: dict[str, Any] | None = None
        self._id: int | str | None = None

    @property
    def result(self) -> T:
        if not self._done:
            raise RuntimeError("batch has not been sent yet")
        if self._error is not None:
            raise RpcError(
                self._error["code"],
                self._error["message"],
                self._error.get("data"),
                id=self._id,
            )
        return self._value  # type: ignore[return-value]

    def _resolve(self, response: dict[str, Any], decode: Callable[[Any], T]) -> None:
        self._id = response.get("id")
        if "error" in response:
            self._error = response["error"]
        else:
            self._value = decode(response["result"])
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
        request: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            request["params"] = _to_jsonable(params)
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
        request: dict[str, Any] = {"jsonrpc": "2.0", "method": method, "id": req_id}
        if params is not None:
            request["params"] = _to_jsonable(params)
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
        by_id: dict[Any, dict[str, Any]] = {}
        if response is not None:
            if not isinstance(response, list):
                raise TypeError("batch response must be a JSON array")
            for item in response:
                by_id[item["id"]] = item

        for request, slot in zip(self._requests, self._slots, strict=True):
            if slot is None:
                continue
            pending, decode = slot
            req_id = request["id"]
            if req_id not in by_id:
                raise LookupError(f"missing batch response for id={req_id!r}")
            pending._resolve(by_id[req_id], decode)

        self._sent = True

    def _ensure_open(self) -> None:
        if self._sent:
            raise RuntimeError("cannot enqueue on a sent batch")


_active_batch: ContextVar[_BatchSession | None] = ContextVar(
    "airbus_rpc_batch", default=None
)


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
    ) -> None:
        self._client = client
        self._queue = queue
        self._on_event = on_event
        self._host = host
        self._exhaustion_timeout_ms = exhaustion_timeout_ms
        self._max_retries = max_retries

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
                    doc = json.loads(body)
                    req_id = doc.get("id", 1)
                    params = doc.get("params", {})
                    try:
                        self.events.append(params)
                        self._on_event(params)
                        res = {
                            "jsonrpc": "2.0",
                            "result": {"status": "ok"},
                            "id": req_id,
                        }
                    except Exception as e:
                        res = {
                            "jsonrpc": "2.0",
                            "error": {"code": -32603, "message": str(e)},
                            "id": req_id,
                        }
                    conn.sendall(json.dumps(res).encode())
                    try:
                        conn.shutdown(socket.SHUT_WR)
                    except OSError:
                        pass
            except Exception:
                pass
            finally:
                conn.close()

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
                self._client.detach_listener(
                    DetachListenerParams(listener_id=self._listener_id)
                )
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
    def __init__(self, host: str, port: int, timeout: float = 2.0) -> None:
        self.host = host
        self.port = port
        self.timeout = timeout

    def _raw(self, payload: Any) -> Any | None:
        return self._raw_text(json.dumps(payload))

    def _raw_text(self, text: str) -> Any | None:
        with socket.create_connection(
            (self.host, self.port), timeout=self.timeout
        ) as sock:
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

    def call(
        self, method: str, params: Any = None, id: int | str = 1
    ) -> Any | Pending[Any]:
        batch = _active_batch.get()
        if batch is not None:
            return batch.enqueue_call(method, params, lambda result: result)

        request: dict[str, Any] = {"jsonrpc": "2.0", "method": method, "id": id}
        if params is not None:
            request["params"] = _to_jsonable(params)
        return self._raw(request)

    def notify(self, method: str, params: Any = None) -> None:
        batch = _active_batch.get()
        if batch is not None:
            batch.enqueue_notify(method, params)
            return

        request: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            request["params"] = _to_jsonable(params)
        if self._raw(request) is not None:
            raise AssertionError("JSON-RPC notification must not produce a response")

    def ping(self) -> PingResult | Pending[PingResult]:
        return self._invoke("ping", None, _decode_ping)

    def add(self, params: AddParams) -> AddResult | Pending[AddResult]:
        return self._invoke("add", params, _decode_add)

    def create_queue(
        self, params: CreateQueueParams
    ) -> CreateQueueResult | Pending[CreateQueueResult]:
        return self._invoke(
            "create_queue",
            _create_queue_wire_params(params),
            _from_create_queue_result,
        )

    def post_event(
        self, params: PostEventParams
    ) -> PostEventResult | Pending[PostEventResult]:
        return self._invoke("post_event", params, _from_post_event_result)

    def get_events(
        self, params: GetEventsParams
    ) -> GetEventsResult | Pending[GetEventsResult]:
        return self._invoke(
            "get_events",
            _get_events_wire_params(params),
            _from_get_events_result,
        )

    def list_queues(self) -> ListQueuesResult | Pending[ListQueuesResult]:
        return self._invoke("list_queues", None, _from_list_queues_result)

    def peek_events(
        self, params: PeekEventsParams
    ) -> PeekEventsResult | Pending[PeekEventsResult]:
        return self._invoke(
            "peek_events",
            _peek_events_wire_params(params),
            _from_peek_events_result,
        )

    def attach_listener(
        self, params: AttachListenerParams
    ) -> AttachListenerResult | Pending[AttachListenerResult]:
        return self._invoke(
            "attach_listener",
            _attach_listener_wire_params(params),
            _from_attach_listener_result,
        )

    def detach_listener(
        self, params: DetachListenerParams
    ) -> DetachListenerResult | Pending[DetachListenerResult]:
        return self._invoke(
            "detach_listener",
            _detach_listener_wire_params(params),
            _from_detach_listener_result,
        )

    def list_listeners(
        self, params: ListListenersParams | None = None
    ) -> ListListenersResult | Pending[ListListenersResult]:
        return self._invoke(
            "list_listeners",
            _list_listeners_wire_params(params),
            _from_list_listeners_result,
        )

    def listen(
        self,
        queue: str,
        on_event: Callable[[dict[str, Any]], Any] | None = None,
        *,
        host: str = "127.0.0.1",
        exhaustion_timeout_ms: int | None = None,
        max_retries: int | None = None,
    ) -> EventListener:
        cb = on_event if on_event is not None else (lambda e: None)
        return EventListener(
            self,
            queue,
            cb,
            host=host,
            exhaustion_timeout_ms=exhaustion_timeout_ms,
            max_retries=max_retries,
        )

    def _invoke(
        self,
        method: str,
        params: Any,
        decode: Callable[[Any], T],
    ) -> T | Pending[T]:
        batch = _active_batch.get()
        if batch is not None:
            return batch.enqueue_call(method, params, decode)

        response = self.call(method, params=params)
        assert not isinstance(response, Pending)
        if "error" in response:
            raise RpcError(
                response["error"]["code"],
                response["error"]["message"],
                response["error"].get("data"),
                id=response.get("id"),
            )
        return decode(response["result"])
