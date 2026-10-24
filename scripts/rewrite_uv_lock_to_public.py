#!/usr/bin/env python3
"""Rewrite uv.lock download URLs to public PyPI without changing resolved versions.

Verigate (and similar mirrors) embed private hosts in the lockfile. A fresh
`uv lock` would re-resolve ranges and float versions. This script instead:

  - keeps every package name/version/hash exactly as locked
  - looks up each sha256 on https://pypi.org
  - replaces registry + artifact URLs with the matching public URLs

If a locked hash is not on PyPI, the script fails (possible private package or
diverging mirror content).

Usage:
  python scripts/rewrite_uv_lock_to_public.py
  python scripts/rewrite_uv_lock_to_public.py path/to/uv.lock
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import urlparse

ALLOWED_HOSTS = frozenset({"pypi.org", "files.pythonhosted.org"})
PUBLIC_REGISTRY = "https://pypi.org/simple"
USER_AGENT = "airbus-rewrite-uv-lock-to-public/1.0"


def host_of(url: str) -> str:
    return (urlparse(url).hostname or "").lower()


def is_public_url(url: str) -> bool:
    return host_of(url) in ALLOWED_HOSTS


def pypi_sha256_urls(name: str, version: str) -> dict[str, str]:
    """Map sha256 digest -> official download URL for one release."""
    req = urllib.request.Request(
        f"https://pypi.org/pypi/{name}/{version}/json",
        headers={"Accept": "application/json", "User-Agent": USER_AGENT},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = json.load(resp)
    except urllib.error.HTTPError as exc:
        raise SystemExit(
            f"PyPI lookup failed for {name}=={version}: HTTP {exc.code}"
        ) from exc
    except urllib.error.URLError as exc:
        raise SystemExit(
            f"PyPI lookup failed for {name}=={version}: {exc.reason}"
        ) from exc

    out: dict[str, str] = {}
    for item in data.get("urls", []):
        digest = (item.get("digests") or {}).get("sha256")
        url = item.get("url")
        if digest and url:
            out[digest] = url
    if not out:
        raise SystemExit(f"PyPI returned no files for {name}=={version}")
    return out


def package_needs_rewrite(pkg: dict) -> bool:
    source = pkg.get("source") or {}
    if "registry" in source and not is_public_url(source["registry"]):
        return True
    sdist = pkg.get("sdist")
    if isinstance(sdist, dict) and sdist.get("url") and not is_public_url(sdist["url"]):
        return True
    for wheel in pkg.get("wheels") or []:
        if isinstance(wheel, dict) and wheel.get("url") and not is_public_url(wheel["url"]):
            return True
    return False


def collect_replacements(lock: dict) -> tuple[dict[str, str], list[str]]:
    """Return (url_old->url_new, registry_urls_to_replace)."""
    url_map: dict[str, str] = {}
    registries: list[str] = []

    for pkg in lock.get("package") or []:
        if not package_needs_rewrite(pkg):
            continue
        name = pkg.get("name")
        version = pkg.get("version")
        if not name or not version:
            raise SystemExit(f"package entry missing name/version: {pkg!r}")

        source = pkg.get("source") or {}
        reg = source.get("registry")
        if isinstance(reg, str) and not is_public_url(reg) and reg not in registries:
            registries.append(reg)

        by_hash = pypi_sha256_urls(name, version)

        artifacts: list[dict] = []
        if isinstance(pkg.get("sdist"), dict):
            artifacts.append(pkg["sdist"])
        artifacts.extend(
            w for w in (pkg.get("wheels") or []) if isinstance(w, dict)
        )

        for art in artifacts:
            url = art.get("url")
            raw_hash = art.get("hash") or ""
            if not url or is_public_url(url):
                continue
            if not raw_hash.startswith("sha256:"):
                raise SystemExit(
                    f"{name}=={version}: artifact hash is not sha256: {raw_hash!r}"
                )
            digest = raw_hash.removeprefix("sha256:")
            public = by_hash.get(digest)
            if public is None:
                raise SystemExit(
                    f"{name}=={version}: sha256 {digest} not found on PyPI "
                    "(refusing to rewrite; mirror may have diverged)"
                )
            url_map[url] = public

    return url_map, registries


def apply_replacements(text: str, url_map: dict[str, str], registries: list[str]) -> str:
    # Longest-first so overlapping prefixes cannot partially rewrite.
    for old in sorted(url_map, key=len, reverse=True):
        new = url_map[old]
        if old not in text:
            raise SystemExit(f"locked URL missing from file text: {old}")
        text = text.replace(old, new)

    for reg in registries:
        if reg not in text:
            raise SystemExit(f"locked registry missing from file text: {reg}")
        text = text.replace(reg, PUBLIC_REGISTRY)

    return text


def find_non_public_hosts(text: str) -> list[str]:
    hosts: set[str] = set()
    for match in re.finditer(r"https?://[^\s\"']+", text):
        host = host_of(match.group(0))
        if host and host not in ALLOWED_HOSTS:
            hosts.add(host)
    return sorted(hosts)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "lockfile",
        nargs="?",
        type=Path,
        default=Path("uv.lock"),
        help="path to uv.lock (default: ./uv.lock)",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="exit 1 if any non-public hosts remain (no writes)",
    )
    args = parser.parse_args()
    path: Path = args.lockfile
    if not path.is_file():
        print(f"error: {path} not found", file=sys.stderr)
        return 1

    text = path.read_text(encoding="utf-8")
    bad = find_non_public_hosts(text)
    if args.check:
        if bad:
            print("non-public hosts in lockfile:", ", ".join(bad), file=sys.stderr)
            return 1
        print("uv.lock hosts are public PyPI only")
        return 0

    if not bad:
        print("uv.lock already uses public PyPI hosts only")
        return 0

    lock = tomllib.loads(text)
    url_map, registries = collect_replacements(lock)
    if not url_map and not registries:
        print(
            "non-public hosts present but no rewritable registry/artifact URLs found:",
            ", ".join(bad),
            file=sys.stderr,
        )
        return 1

    new_text = apply_replacements(text, url_map, registries)
    remaining = find_non_public_hosts(new_text)
    if remaining:
        print(
            "rewrite incomplete; still non-public:",
            ", ".join(remaining),
            file=sys.stderr,
        )
        return 1

    if new_text == text:
        print("no textual changes after rewrite", file=sys.stderr)
        return 1

    path.write_text(new_text, encoding="utf-8")
    print(
        f"rewrote {len(url_map)} artifact URL(s) and "
        f"{len(registries)} registry URL(s) in {path}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
