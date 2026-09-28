"""JSON-RPC envelopes and typed payload codecs; no sockets or threads.

Runtime JSON Schema validation lives on the daemon. Clients encode/decode
generated payload types and check JSON-RPC envelopes only.
"""

from __future__ import annotations

import json
import types
from collections.abc import Callable
from dataclasses import fields, is_dataclass
from enum import Enum
from typing import Any, get_args, get_origin, get_type_hints

from airbus_client import payloads

VERSION = "2.0"
_ABSENT = object()


class RpcError(Exception):
    def __init__(self, code: int, message: str, data: Any = None, *, id: int | str | None = None):
        super().__init__(f"{code}: {message}")
        self.code, self.message, self.data, self.id = code, message, data, id


def to_wire(value: Any) -> Any:
    if is_dataclass(value) and not isinstance(value, type):
        # Omit absent optional fields, but preserve nulls inside user event dictionaries.
        return {
            field.name: to_wire(item)
            for field in fields(value)
            if (item := getattr(value, field.name)) is not None
        }
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, dict):
        return {key: to_wire(item) for key, item in value.items()}
    if isinstance(value, list):
        return [to_wire(item) for item in value]
    return value


def _decode(model: Any, value: Any) -> Any:
    if value is None:
        return None
    origin, args = get_origin(model), get_args(model)
    if origin is types.UnionType:
        model = next(choice for choice in args if choice is not type(None))
        return _decode(model, value)
    if origin is list:
        return [_decode(args[0], item) for item in value]
    if is_dataclass(model):
        hints = get_type_hints(model)
        return model(
            **{
                field.name: _decode(hints[field.name], value[field.name])
                for field in fields(model)
                if field.name in value
            }
        )
    if isinstance(model, type) and issubclass(model, Enum):
        return model(value)
    if model is float:
        return float(value)
    return value


def decode_result(method: str, model: Any, value: Any) -> Any:
    del method  # retained for call-site parity with older clients
    return _decode(model, value)


def request(method: str, params: Any = None, id: Any = _ABSENT) -> dict[str, Any]:
    document = {"jsonrpc": VERSION, "method": method}
    if id is not _ABSENT:
        document["id"] = id
    if params is not None:
        document["params"] = to_wire(params)
    return document


def _valid_id(value: Any) -> bool:
    return value is None or type(value) in (int, str)


def validate_response(response: Any, expected_id: Any) -> dict[str, Any]:
    if not isinstance(response, dict) or response.get("jsonrpc") != VERSION:
        raise ValueError("invalid JSON-RPC response version")
    if "id" not in response or not _valid_id(response["id"]):
        raise ValueError("invalid JSON-RPC response id")
    if type(response["id"]) is not type(expected_id) or response["id"] != expected_id:
        raise ValueError("JSON-RPC response id does not match request")
    if ("result" in response) == ("error" in response):
        raise ValueError("response must contain exactly one of result or error")
    if "error" in response:
        error = response["error"]
        if (
            not isinstance(error, dict)
            or type(error.get("code")) is not int
            or not isinstance(error.get("message"), str)
        ):
            raise ValueError("invalid JSON-RPC error object")
    return response


def response_result(response: dict[str, Any]) -> Any:
    if "error" in response:
        error = response["error"]
        raise RpcError(error["code"], error["message"], error.get("data"), id=response["id"])
    return response["result"]


def batch_responses(requests: list[dict], response: Any) -> list[dict | None]:
    expected = {item["id"] for item in requests if "id" in item}
    if not expected:
        if response is not None:
            raise ValueError("notifications must not receive a response")
        return [None] * len(requests)
    if not isinstance(response, list):
        raise ValueError("batch response must be an array")
    by_id = {}
    for item in response:
        if not isinstance(item, dict) or not _valid_id(item.get("id")):
            raise ValueError("invalid batch response")
        id = item.get("id")
        if id not in expected or id in by_id:
            raise ValueError("unexpected or duplicate batch response id")
        by_id[id] = validate_response(item, next(value for value in expected if value == id))
    if set(by_id) != expected:
        raise ValueError("missing batch response")
    return [by_id[item["id"]] if "id" in item else None for item in requests]


def _error_response(id: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": VERSION, "id": id, "error": {"code": code, "message": message}}


def handle_event(body: str, callback: Callable[[dict[str, Any]], Any]) -> dict[str, Any] | None:
    try:
        document = json.loads(body)
    except ValueError:
        return _error_response(None, -32700, "invalid JSON")
    if not isinstance(document, dict):
        return _error_response(None, -32600, "invalid request")
    id = document.get("id")
    if (
        document.get("jsonrpc") != VERSION
        or not _valid_id(id)
        or not isinstance(document.get("method"), str)
    ):
        return _error_response(id if _valid_id(id) else None, -32600, "invalid request")
    notification = "id" not in document
    if document["method"] != "on_event":
        return None if notification else _error_response(id, -32601, "method not found")
    try:
        event = _decode(payloads.ListenerEventParams, document["params"])
    except (KeyError, TypeError, ValueError) as error:
        return None if notification else _error_response(id, -32602, str(error))
    try:
        callback(to_wire(event))
    except Exception as error:
        return None if notification else _error_response(id, -32603, str(error))
    if notification:
        return None
    acknowledgment = payloads.ListenerEventResult(status=payloads.Status.ok)
    return {"jsonrpc": VERSION, "id": id, "result": to_wire(acknowledgment)}
