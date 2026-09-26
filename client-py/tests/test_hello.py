"""Integration tests against a live Airbus process over TCP."""

import subprocess

from airbus_client.binary import airbus_bin


def test_requires_listen() -> None:
    result = subprocess.run(
        [str(airbus_bin())],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 1
    assert "[info] Airbus!" in result.stderr
    assert "usage: airbus --listen" in result.stderr
