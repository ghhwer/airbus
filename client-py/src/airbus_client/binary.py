import os
import subprocess
from pathlib import Path


def airbus_bin() -> Path:
    env = os.environ.get("AIRBUS_BIN")
    if env:
        return Path(env)
    return Path(__file__).resolve().parents[3] / "out" / "airbus"


def run_airbus(*args: str, input_text: str | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(airbus_bin()), *args],
        input=input_text,
        capture_output=True,
        text=True,
        check=True,
    )
