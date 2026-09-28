"""Architectural guards and black-box protocol contract regressions."""

import importlib.util
import json
import re
from pathlib import Path

import pytest

from airbus_client.rpc import RpcClient

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "protocol_boundaries", ROOT / "scripts/check_protocol_boundaries.py"
)
boundaries = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundaries)


def test_protocol_boundaries():
    assert boundaries.check() == []


@pytest.mark.parametrize(
    "filename,source",
    [
        ("extra.py", 'packet = {"jsonrpc": "2.0"}'),
        ("extra.py", 'result = response["result"]'),
        ("extra.rs", 'let packet = serde_json::json!({"jsonrpc": "2.0"});'),
        ("extra.rs", 'let result = response.get("result");'),
        ("extra.js", 'const packet = {jsonrpc: "2.0"};'),
    ],
)
def test_guard_rejects_protocol_code_in_new_modules(filename, source):
    assert boundaries.violations(Path(filename), source)


def test_guard_allows_test_fixtures_but_checks_production_after_them():
    fixture = (
        '#[cfg(test)] mod tests { fn fixture() { let v = serde_json::json!({"jsonrpc":"2.0"}); } }'
    )
    assert not boundaries.violations(Path("example.rs"), fixture)
    assert boundaries.violations(
        Path("example.rs"), fixture + '\nfn production() { response.get("result"); }'
    )


def test_guard_allows_arbitrary_event_json():
    assert not boundaries.violations(
        Path("example.rs"), 'let event = serde_json::json!({"task": "run"});'
    )


def test_url_strings_do_not_hide_boundary_violations():
    assert boundaries.violations(
        Path("example.rs"), 'let url = "http://local"; response.get("result");'
    )


def test_callback_is_in_the_method_catalog():
    catalog = json.loads((ROOT / "schema/openrpc.json").read_text())
    callback = next((method for method in catalog["methods"] if method["name"] == "on_event"), None)
    assert callback is not None, "on_event and its acknowledgment must be schema-defined"
    assert callback["x-receiver"] == "listener"
    assert (ROOT / "schema" / callback["result"]["schema"]["$ref"]).is_file()


@pytest.mark.parametrize(
    "response",
    [
        {"id": 1, "result": "pong"},
        {"jsonrpc": "1.0", "id": 1, "result": "pong"},
        {"jsonrpc": "2.0", "id": 2, "result": "pong"},
        {"jsonrpc": "2.0", "id": True, "result": "pong"},
        {"jsonrpc": "2.0", "id": 1, "result": "pong", "error": {}},
        {"jsonrpc": "2.0", "id": 1, "error": {"code": "bad", "message": 123}},
    ],
)
def test_client_rejects_invalid_response_envelopes(monkeypatch, response):
    client = RpcClient("127.0.0.1", 1)
    monkeypatch.setattr(client, "_raw", lambda _: response)
    with pytest.raises(ValueError):
        client.call("ping")


def test_generated_result_decode_rejects_invalid_enum(monkeypatch):
    client = RpcClient("127.0.0.1", 1)
    monkeypatch.setattr(
        client,
        "_raw",
        lambda _: {
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"queue": "demo", "mode": "not-a-mode", "created": True},
        },
    )
    from airbus_client.payloads import CreateQueueParams

    with pytest.raises(ValueError):
        client.create_queue(CreateQueueParams(queue="demo"))


@pytest.mark.parametrize(
    "document",
    [
        {"id": 1, "method": "on_event", "params": {"queue": "demo", "id": "event", "event": {}}},
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "wrong",
            "params": {"queue": "demo", "id": "event", "event": {}},
        },
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "on_event",
            "params": {"queue": "demo", "id": "event"},
        },
    ],
)
def test_listener_rejects_invalid_requests_before_callback(rpc, document):
    from airbus_client.payloads import CreateQueueParams

    events = []
    rpc.create_queue(CreateQueueParams(queue="demo"))
    with rpc.listen("demo", on_event=events.append) as listener:
        response = RpcClient("127.0.0.1", listener.port)._raw(document)
        assert events == []
        assert "error" in response


@pytest.mark.parametrize(
    "responses",
    [
        [{"jsonrpc": "2.0", "id": 2, "result": "pong"}],
        [{"jsonrpc": "2.0", "id": 1, "result": "pong"}] * 2,
        [{"id": 1, "result": "pong"}],
        [],
    ],
)
def test_batch_rejects_unmatched_duplicate_or_malformed_responses(monkeypatch, responses):
    client = RpcClient("127.0.0.1", 1)
    monkeypatch.setattr(client, "_raw", lambda _: responses)
    with pytest.raises(ValueError), client.batch():
        client.ping()


def test_codec_preserves_user_nulls_and_decodes_nested_models():
    from airbus_client import protocol
    from airbus_client.payloads import ListQueuesResult, PostEventParams, QueueInfo, QueueMode

    value = protocol.to_wire(PostEventParams(queue="demo", event={"optional": None}))
    assert value["event"] == {"optional": None}
    result = protocol.decode_result(
        "list_queues",
        ListQueuesResult,
        {
            "queues": [{"name": "demo", "depth": 0, "mode": "broadcast", "listener_count": 1}],
        },
    )
    assert isinstance(result.queues[0], QueueInfo)
    assert result.queues[0].mode is QueueMode.broadcast


def test_openrpc_methods_match_server_routes_and_client_methods():
    catalog = json.loads((ROOT / "schema/openrpc.json").read_text())
    server_methods = {
        method["name"]
        for method in catalog["methods"]
        if method.get("x-receiver", "server") == "server"
    }
    routes = set(re.findall(r'server\.route\("([^"]+)"', (ROOT / "src/wiring.rs").read_text()))
    assert routes == server_methods
    assert all(callable(getattr(RpcClient, name, None)) for name in server_methods)


def test_optional_method_parameters_may_be_omitted(rpc):
    assert rpc.list_listeners().listeners == []
