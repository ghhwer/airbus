"""Resolve the Airbus TCP endpoint from the environment.

Precedence: ``AIRBUS_URL`` (``host:port``), else ``AIRBUS_HOST`` / ``AIRBUS_PORT``,
else ``127.0.0.1:9097``.
"""

from __future__ import annotations

import os

_DEFAULT_HOST = "127.0.0.1"
_DEFAULT_PORT = 9097


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
