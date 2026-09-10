from __future__ import annotations

import json
import socket
from collections.abc import Callable
from contextlib import AbstractContextManager
from contextvars import ContextVar, Token
from dataclasses import asdict, is_dataclass
from typing import Any, Generic, TypeVar

from airbus_client.payloads import (
    AddParams,
    AddResult,
    GetEventsParams,
    GetEventsResult,
    PingResult,
    PostEventParams,
    PostEventResult,
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


def _get_events_wire_params(params: GetEventsParams) -> dict[str, Any]:
    payload: dict[str, Any] = {"queue": params.queue}
    if params.count is not None:
        payload["count"] = params.count
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


class RpcClient:
    def __init__(self, host: str, port: int, timeout: float = 2.0) -> None:
        self.host = host
        self.port = port
        self.timeout = timeout

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
        return decode(response["result"])
