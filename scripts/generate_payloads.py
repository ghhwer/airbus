#!/usr/bin/env python3
"""Generate Rust + Python payload types from airbus/schema/payloads."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PAYLOADS = ROOT / "schema" / "payloads"
COMMON_NAME = "common.schema.json"
RS_OUT = ROOT / "src" / "proto" / "payloads.rs"
PY_OUT = ROOT / "client" / "src" / "airbus_client" / "payloads.py"

PAYLOAD_FILES = [
    "ping_result.schema.json",
    "add_params.schema.json",
    "add_result.schema.json",
    "post_event_params.schema.json",
    "post_event_result.schema.json",
    "get_events_params.schema.json",
    "get_events_result.schema.json",
    "list_queues_result.schema.json",
    "peek_events_params.schema.json",
    "peek_events_result.schema.json",
    "create_queue_params.schema.json",
    "create_queue_result.schema.json",
    "attach_listener_params.schema.json",
    "attach_listener_result.schema.json",
    "detach_listener_params.schema.json",
    "detach_listener_result.schema.json",
    "list_listeners_params.schema.json",
    "list_listeners_result.schema.json",
    "listener_event_params.schema.json",
]


def load(name: str) -> dict:
    return json.loads((PAYLOADS / name).read_text())


def rewrite_refs_rust(node: object) -> object:
    if isinstance(node, dict):
        if set(node.keys()) == {"$ref"}:
            ref = node["$ref"]
            prefix = f"{COMMON_NAME}#/$defs/"
            if ref.startswith(prefix):
                return {"$ref": f"#/$defs/{ref[len(prefix):]}"}
            return node
        return {
            k: rewrite_refs_rust(v)
            for k, v in node.items()
            if k not in ("$schema", "$id")
        }
    if isinstance(node, list):
        return [rewrite_refs_rust(x) for x in node]
    return node


def rewrite_refs_python(node: object, common_defs: dict) -> object:
    """Inline common $defs for strings/objects; preserve shared enums."""
    if isinstance(node, dict):
        if set(node.keys()) == {"$ref"}:
            ref = node["$ref"]
            prefix = f"{COMMON_NAME}#/$defs/"
            if ref.startswith(prefix):
                key = ref[len(prefix) :]
                if key in ("QueueMode", "DispatchStrategy"):
                    return {"$ref": f"#/$defs/{key}"}
                inlined = json.loads(json.dumps(common_defs[key]))  # deep copy
                inlined.pop("title", None)
                return rewrite_refs_python(inlined, common_defs)
            return node
        return {
            k: rewrite_refs_python(v, common_defs)
            for k, v in node.items()
            if k not in ("$schema", "$id")
        }
    if isinstance(node, list):
        return [rewrite_refs_python(x, common_defs) for x in node]
    return node


def write_rust_bundle() -> Path:
    common = load(COMMON_NAME)
    defs: dict = {}
    for key, value in common["$defs"].items():
        defs[key] = rewrite_refs_rust(value)

    for fname in PAYLOAD_FILES:
        doc = load(fname)
        title = doc["title"]
        body = {k: v for k, v in doc.items() if k not in ("$schema", "$id", "title")}
        defs[title] = rewrite_refs_rust(body)

    bundle = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://airbus.local/schema/payloads/bundle.schema.json",
        "title": "AirbusPayloadRoot",
        "$defs": defs,
        "oneOf": [{"$ref": f"#/$defs/{name}"} for name in defs],
    }
    out = PAYLOADS / "bundle.schema.json"
    out.write_text(json.dumps(bundle, indent=2) + "\n")
    return out


def write_python_bundle() -> Path:
    common = load(COMMON_NAME)
    common_defs = common["$defs"]
    defs: dict = {}
    props: dict = {}
    for enum_key in ("QueueMode", "DispatchStrategy"):
        if enum_key in common_defs:
            defs[enum_key] = common_defs[enum_key]
            props[enum_key] = {"$ref": f"#/$defs/{enum_key}"}

    for fname in PAYLOAD_FILES:
        doc = load(fname)
        title = doc["title"]
        body = {k: v for k, v in doc.items() if k not in ("$schema", "$id", "title")}
        defs[title] = rewrite_refs_python(body, common_defs)
        props[title] = {"$ref": f"#/$defs/{title}"}

    # Root object with a property per payload forces every $def into the module.
    bundle = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://airbus.local/schema/payloads/python_bundle.schema.json",
        "title": "AirbusPayloadModels",
        "type": "object",
        "additionalProperties": False,
        "$defs": defs,
        "properties": props,
    }
    out = PAYLOADS / "python_bundle.schema.json"
    out.write_text(json.dumps(bundle, indent=2) + "\n")
    return out


def generate_rust(bundle: Path) -> None:
    cargo = shutil.which("cargo")
    if cargo is None:
        cargo_home = Path.home() / ".cargo" / "bin" / "cargo"
        if cargo_home.exists():
            cargo = str(cargo_home)
        else:
            raise SystemExit("cargo not found on PATH")

    RS_OUT.parent.mkdir(parents=True, exist_ok=True)
    header = (
        "// @generated by scripts/generate_payloads.py — do not edit by hand.\n"
        "// Source of truth: schema/payloads/*.schema.json\n"
        "#![allow(clippy::redundant_closure_call)]\n"
        "#![allow(clippy::needless_lifetimes)]\n"
        "#![allow(clippy::match_single_binding)]\n"
        "#![allow(clippy::clone_on_copy)]\n\n"
    )
    subprocess.run(
        [
            cargo,
            "typify",
            "-B",
            "-o",
            str(RS_OUT),
            str(bundle),
        ],
        check=True,
        cwd=ROOT,
    )
    body = RS_OUT.read_text()
    # typify already emits allow attrs; replace with our header + body without dup allows
    lines = body.splitlines(keepends=True)
    while lines and (
        lines[0].startswith("#![allow")
        or lines[0].startswith("//")
        or lines[0].strip() == ""
    ):
        # keep typify allows by skipping only blank; we'll prepend ours
        if lines[0].startswith("#![allow"):
            lines.pop(0)
            continue
        if lines[0].strip() == "":
            lines.pop(0)
            continue
        break
    RS_OUT.write_text(header + "".join(lines))
    print(f"wrote {RS_OUT.relative_to(ROOT)}", file=sys.stderr)


def _strip_python_root_model(text: str) -> str:
    """Remove the synthetic AirbusPayloadModels root used only to force codegen."""
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("@dataclass") and i + 1 < len(lines) and lines[
            i + 1
        ].startswith("class AirbusPayloadModels"):
            i += 1
            while i < len(lines) and (
                lines[i].startswith("class AirbusPayloadModels")
                or lines[i].startswith("    ")
            ):
                i += 1
            while i < len(lines) and lines[i].strip() == "":
                i += 1
            continue
        if line.startswith("class AirbusPayloadModels"):
            i += 1
            while i < len(lines) and lines[i].startswith("    "):
                i += 1
            while i < len(lines) and lines[i].strip() == "":
                i += 1
            continue
        out.append(line)
        i += 1
    return "".join(out)


def generate_python(bundle: Path) -> None:
    client = ROOT / "client"
    PY_OUT.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "uv",
            "run",
            "datamodel-codegen",
            "--input",
            str(bundle),
            "--input-file-type",
            "jsonschema",
            "--output-model-type",
            "dataclasses.dataclass",
            "--target-python-version",
            "3.13",
            "--use-standard-collections",
            "--use-union-operator",
            "--collapse-root-models",
            "--formatters",
            "builtin",
            "--output",
            str(PY_OUT),
        ],
        check=True,
        cwd=client,
    )
    raw_lines = []
    for line in PY_OUT.read_text().splitlines(keepends=True):
        if line.startswith("# generated by datamodel-codegen:"):
            continue
        if line.startswith("#   filename:"):
            continue
        if line.startswith("#   timestamp:"):
            continue
        raw_lines.append(line)
    text = _strip_python_root_model("".join(raw_lines))

    aliases = [
        ("PingResult", "str"),
        ("AddResult", "float"),
    ]
    missing = [
        (name, ty)
        for name, ty in aliases
        if f"type {name} =" not in text and f"class {name}" not in text
    ]
    header = (
        "# @generated by scripts/generate_payloads.py — do not edit by hand.\n"
        "# Source of truth: schema/payloads/*.schema.json\n\n"
    )
    body = text.lstrip("\n")
    if missing:
        lines = body.splitlines(keepends=True)
        last_import = -1
        for idx, line in enumerate(lines):
            s = line.strip()
            if s.startswith("from ") or s.startswith("import "):
                last_import = idx
                continue
            if last_import >= 0 and s == "":
                continue
            if last_import >= 0:
                break
        if last_import < 0:
            raise SystemExit("python codegen: expected import block")
        insert_at = last_import + 1
        while insert_at < len(lines) and lines[insert_at].strip() == "":
            insert_at += 1
        alias_block = (
            "\n".join(f"type {name} = {ty}" for name, ty in missing) + "\n\n"
        )
        lines.insert(insert_at, alias_block)
        body = "".join(lines)

    PY_OUT.write_text(header + body.rstrip() + "\n")
    print(f"wrote {PY_OUT.relative_to(ROOT)}", file=sys.stderr)


def main() -> int:
    rust_bundle = write_rust_bundle()
    print(f"wrote {rust_bundle.relative_to(ROOT)}", file=sys.stderr)
    generate_rust(rust_bundle)

    py_bundle = write_python_bundle()
    print(f"wrote {py_bundle.relative_to(ROOT)}", file=sys.stderr)
    generate_python(py_bundle)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
