#!/usr/bin/env python3
"""Assemble C++ and PlatformIO release zip assets from client-cpp."""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CPP_DIR = ROOT / "client-cpp"
PIO_LIBRARY_TEMPLATE = ROOT / "packaging" / "platformio" / "library.json"
NLOHMANN_URL = (
    "https://raw.githubusercontent.com/nlohmann/json/v3.11.3/"
    "single_include/nlohmann/json.hpp"
)


def read_version(explicit: str | None) -> str:
    if explicit:
        return explicit.lstrip("v")
    version = (CPP_DIR / "VERSION").read_text().strip()
    if not version:
        raise SystemExit("client-cpp/VERSION is empty")
    return version


def fetch_nlohmann(dest_dir: Path) -> None:
    dest = dest_dir / "nlohmann" / "json.hpp"
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_file() and dest.stat().st_size > 0:
        return
    print(f"fetching nlohmann/json → {dest}", file=sys.stderr)
    with urllib.request.urlopen(NLOHMANN_URL, timeout=60) as response:
        dest.write_bytes(response.read())


def copy_cpp_sources(dest: Path) -> None:
    for name in ("include", "src", "examples", "CMakeLists.txt", "README.md", "VERSION"):
        src = CPP_DIR / name
        target = dest / name
        if src.is_dir():
            shutil.copytree(src, target, dirs_exist_ok=True)
        else:
            shutil.copy2(src, target)


def write_zip(source_dir: Path, zip_path: Path) -> None:
    zip_path.parent.mkdir(parents=True, exist_ok=True)
    if zip_path.exists():
        zip_path.unlink()
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(source_dir.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(source_dir.parent).as_posix())


def pack_cpp(staging: Path, version: str, out_dir: Path) -> Path:
    root_name = f"airbus-client-cpp-v{version}"
    dest = staging / root_name
    dest.mkdir(parents=True)
    copy_cpp_sources(dest)
    fetch_nlohmann(dest / "third_party")
    zip_path = out_dir / f"{root_name}.zip"
    write_zip(dest, zip_path)
    return zip_path


def pack_pio(staging: Path, version: str, out_dir: Path) -> Path:
    """Same client-cpp tree, plus a PlatformIO library.json (packaging only)."""
    root_name = f"airbus-client-pio-v{version}"
    dest = staging / root_name
    dest.mkdir(parents=True)

    for name in ("include", "src", "examples", "README.md", "VERSION"):
        src = CPP_DIR / name
        target = dest / name
        if src.is_dir():
            shutil.copytree(src, target)
        else:
            shutil.copy2(src, target)
    fetch_nlohmann(dest / "third_party")

    library = json.loads(PIO_LIBRARY_TEMPLATE.read_text())
    library["version"] = version
    (dest / "library.json").write_text(json.dumps(library, indent=2) + "\n")

    zip_path = out_dir / f"{root_name}.zip"
    write_zip(dest, zip_path)
    return zip_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--version",
        help="Version without leading v (default: client-cpp/VERSION)",
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
        staging = Path(tmp)
        cpp_zip = pack_cpp(staging, version, out_dir)
        pio_zip = pack_pio(staging, version, out_dir)

    print(cpp_zip)
    print(pio_zip)


if __name__ == "__main__":
    main()
