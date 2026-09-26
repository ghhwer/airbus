from __future__ import annotations

import os
import re
import select
import subprocess
import time
from collections.abc import Iterator
from contextlib import contextmanager

from airbus_client.binary import airbus_bin
from airbus_client.rpc import RpcClient

_LISTEN_RE = re.compile(r"listening on ([\d.]+):(\d+)")


@contextmanager
def start_server(bind: str = "127.0.0.1:0", timeout: float = 2.0) -> Iterator[RpcClient]:
    proc = subprocess.Popen(
        [str(airbus_bin()), "--listen", bind],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=False,
    )
    try:
        host, port = _wait_for_listen(proc, timeout)
        yield RpcClient(host, port, timeout=timeout)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()


def _wait_for_listen(proc: subprocess.Popen[bytes], timeout: float) -> tuple[str, int]:
    assert proc.stderr is not None
    fd = proc.stderr.fileno()
    buf = ""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            rest = proc.stderr.read().decode(errors="replace")
            raise RuntimeError(f"airbus exited before listen ({proc.returncode}): {buf}{rest}")
        remaining = deadline - time.monotonic()
        ready, _, _ = select.select([fd], [], [], remaining)
        if not ready:
            break
        chunk = os.read(fd, 4096)
        if not chunk:
            break
        buf += chunk.decode()
        for line in buf.splitlines():
            match = _LISTEN_RE.search(line)
            if match:
                return match.group(1), int(match.group(2))
    raise TimeoutError(f"timed out waiting for airbus listen line: {buf!r}")
