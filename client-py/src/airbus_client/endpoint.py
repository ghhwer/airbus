"""Resolve the Airbus TCP endpoint from the environment.

Precedence: ``AIRBUS_URL`` (``host:port``), else ``AIRBUS_HOST`` / ``AIRBUS_PORT``,
else ``127.0.0.1:9097``.

Listen-host resolution for push callbacks: ``AIRBUS_LISTEN_HOST`` or an explicit
override wins; otherwise the local egress IP toward the Airbus peer is detected
(UDP ``connect`` + ``getsockname``). Loopback peers keep ``127.0.0.1``.
"""

from __future__ import annotations

import os
import socket

_DEFAULT_HOST = "127.0.0.1"
_DEFAULT_PORT = 9097
_LOOPBACK_NAMES = frozenset({"localhost", "127.0.0.1", "::1"})


def parse_endpoint(url: str) -> tuple[str, int]:
    """Parse ``host:port`` into a host/port pair."""
    host, _, port_s = url.rpartition(":")
    if not host or not port_s.isdigit():
        raise ValueError(f"Airbus endpoint must be host:port (got {url!r}); example: 127.0.0.1:9097")
    return host, int(port_s)


def airbus_endpoint() -> tuple[str, int]:
    """Resolve host/port from ``AIRBUS_URL`` or ``AIRBUS_HOST`` / ``AIRBUS_PORT``."""
    url = os.environ.get("AIRBUS_URL", "").strip()
    if url:
        return parse_endpoint(url)
    host = os.environ.get("AIRBUS_HOST", _DEFAULT_HOST)
    port = int(os.environ.get("AIRBUS_PORT", str(_DEFAULT_PORT)))
    return host, port


def airbus_url() -> str:
    """Return ``host:port`` for the resolved endpoint."""
    host, port = airbus_endpoint()
    return f"{host}:{port}"


def is_loopback_host(host: str) -> bool:
    """True when ``host`` is a conventional IPv4/IPv6 loopback name or address."""
    h = host.strip().lower().strip("[]")
    if h in _LOOPBACK_NAMES:
        return True
    if h.startswith("127."):
        return True
    return False


def detect_egress_ip(peer_host: str, peer_port: int) -> str:
    """Return the local IPv4 address used toward ``peer_host:peer_port``.

    Uses a UDP ``connect`` (no packets sent) then ``getsockname()``, matching the
    usual OS routing-table egress discovery trick.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        try:
            sock.connect((peer_host, peer_port))
        except OSError as exc:
            raise RuntimeError(
                f"could not detect egress IP toward {peer_host}:{peer_port}: {exc}; "
                "set AIRBUS_LISTEN_HOST or pass listen_host= explicitly"
            ) from exc
        ip = sock.getsockname()[0]
    if not ip or ip in ("0.0.0.0", "::"):
        raise RuntimeError(
            f"egress IP toward {peer_host}:{peer_port} is unspecified ({ip!r}); "
            "set AIRBUS_LISTEN_HOST or pass listen_host= explicitly"
        )
    return ip


def resolve_listen_host(
    peer_host: str,
    peer_port: int,
    *,
    override: str | None = None,
) -> str:
    """Pick the host the daemon should dial back to for ``on_event``.

    Precedence:
    1. Explicit ``override`` (unless empty / ``\"auto\"``)
    2. ``AIRBUS_LISTEN_HOST`` env
    3. ``127.0.0.1`` when the Airbus peer is loopback
    4. Detected egress IP toward the peer

    Raises if a remote peer would be registered with a loopback listen host.
    """
    chosen: str | None = None
    if override is not None:
        trimmed = override.strip()
        if trimmed and trimmed.lower() != "auto":
            chosen = trimmed
    if chosen is None:
        env = os.environ.get("AIRBUS_LISTEN_HOST", "").strip()
        if env:
            chosen = env
    if chosen is None:
        if is_loopback_host(peer_host):
            return "127.0.0.1"
        chosen = detect_egress_ip(peer_host, peer_port)

    if is_loopback_host(chosen) and not is_loopback_host(peer_host):
        raise RuntimeError(
            f"listen host {chosen!r} is loopback but Airbus peer "
            f"{peer_host}:{peer_port} is remote; the daemon cannot deliver "
            "on_event callbacks. Set AIRBUS_LISTEN_HOST or pass a routable "
            "listen_host= / with_listen_host(...)."
        )
    return chosen
