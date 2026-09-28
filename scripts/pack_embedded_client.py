#!/usr/bin/env python3
"""Assemble the Arduino/ESP32 PlatformIO release zip asset."""

from __future__ import annotations

import argparse
import json
import shutil
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EMBEDDED_DIR = ROOT / "client-embedded"


def read_version(explicit: str | None) -> str:
    if explicit:
        return explicit.lstrip("v")
    version = (EMBEDDED_DIR / "VERSION").read_text().strip()
    if not version:
        raise SystemExit("client-embedded/VERSION is empty")
    return version


def write_zip(source_dir: Path, zip_path: Path) -> None:
    zip_path.parent.mkdir(parents=True, exist_ok=True)
    if zip_path.exists():
        zip_path.unlink()
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(source_dir.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(source_dir.parent).as_posix())


def pack_arduino_esp32(staging: Path, version: str, out_dir: Path) -> Path:
    if not EMBEDDED_DIR.is_dir():
        raise SystemExit(f"missing {EMBEDDED_DIR}")

    root_name = f"airbus-client-arduino-esp32-v{version}"
    dest = staging / root_name
    dest.mkdir(parents=True)

    for name in ("include", "src", "examples", "README.md", "VERSION"):
        src = EMBEDDED_DIR / name
        target = dest / name
        if not src.exists():
            continue
        if src.is_dir():
            shutil.copytree(src, target)
        else:
            shutil.copy2(src, target)

    (dest / "VERSION").write_text(version + "\n")
    library = json.loads((EMBEDDED_DIR / "library.json").read_text())
    library["version"] = version
    (dest / "library.json").write_text(json.dumps(library, indent=2) + "\n")

    zip_path = out_dir / f"{root_name}.zip"
    write_zip(dest, zip_path)
    return zip_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--version",
        help="Version without leading v (default: client-embedded/VERSION)",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=ROOT / "dist",
        help="Directory for zip assets (default: dist/)",
    )
    args = parser.parse_args()
    version = read_version(args.version)
    out_dir = args.out_dir.resolve()

    with tempfile.TemporaryDirectory(prefix="airbus-client-pack-") as tmp:
        zip_path = pack_arduino_esp32(Path(tmp), version, out_dir)

    print(zip_path)


if __name__ == "__main__":
    main()
