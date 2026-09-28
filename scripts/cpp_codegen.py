"""Generate embedded ArduinoJson payload types from schema."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Callable

ROOT: Path
PAYLOADS: Path
COMMON_NAME: str
PAYLOAD_FILES: list[str]
EMBEDDED_PAYLOADS_OUT: Path

load: Callable[[str], dict]
rewrite_refs_rust: Callable[[object], object]


def _cpp_enum_variant(value: str) -> str:
    parts = value.replace("-", "_").split("_")
    return "".join(p[:1].upper() + p[1:] for p in parts if p)


def _emb_enum(name: str, values: list[str]) -> str:
    lines = [f"enum class {name} {{"]
    for value in values:
        lines.append(f"  {_cpp_enum_variant(value)},")
    lines.append("};")
    lines.append("")
    lines.append(
        f"inline bool toJson(JsonVariant dest, {name} value, String& /*err*/) {{"
    )
    lines.append("  switch (value) {")
    for value in values:
        lines.append(
            f"    case {name}::{_cpp_enum_variant(value)}: dest.set({json.dumps(value)}); return true;"
        )
    lines.append("  }")
    lines.append("  return false;")
    lines.append("}")
    lines.append("")
    lines.append(
        f"inline bool fromJson(JsonVariantConst src, {name}& value, String& err) {{"
    )
    lines.append("  const char* s = src.as<const char*>();")
    lines.append("  if (s == nullptr) { err = \"invalid " + name + "\"; return false; }")
    for value in values:
        lines.append(
            f"  if (strcmp(s, {json.dumps(value)}) == 0) {{ value = {name}::{_cpp_enum_variant(value)}; return true; }}"
        )
    lines.append(f'  err = "invalid {name}";')
    lines.append("  return false;")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def _emb_struct(name: str, fields: list[tuple[str, str, bool]]) -> str:
    """fields: (json_name, emb_type, optional). emb_type may be JsonDocument for free-form objects."""
    lines = [f"struct {name} {{"]
    for pname, ctype, optional in fields:
        if optional:
            lines.append(f"  bool has_{pname} = false;")
        lines.append(f"  {ctype} {pname}{{}};")
    lines.append("};")
    lines.append("")

    lines.append(
        f"inline bool toJson(JsonObject dest, const {name}& value, String& err) {{"
    )
    for pname, ctype, optional in fields:
        key = json.dumps(pname)
        if optional:
            lines.append(f"  if (value.has_{pname}) {{")
            if ctype == "JsonDocument":
                lines.append(f"    if (!dest[{key}].set(value.{pname}.as<JsonVariantConst>())) {{")
                lines.append(f'      err = "encode {pname}";')
                lines.append("      return false;")
                lines.append("    }")
            elif ctype.startswith("std::vector<"):
                inner = ctype[len("std::vector<") : -1]
                lines.append(f"    JsonArray arr = dest[{key}].to<JsonArray>();")
                lines.append(f"    for (const auto& item : value.{pname}) {{")
                if inner in ("double", "std::int64_t", "bool", "String"):
                    lines.append("      arr.add(item);")
                else:
                    lines.append("      JsonObject obj = arr.add<JsonObject>();")
                    lines.append("      if (!toJson(obj, item, err)) return false;")
                lines.append("    }")
            else:
                lines.append(f"    if (!toJson(dest[{key}], value.{pname}, err)) return false;")
            lines.append("  }")
        else:
            if ctype == "JsonDocument":
                lines.append(f"  if (!dest[{key}].set(value.{pname}.as<JsonVariantConst>())) {{")
                lines.append(f'    err = "encode {pname}";')
                lines.append("    return false;")
                lines.append("  }")
            elif ctype.startswith("std::vector<"):
                inner = ctype[len("std::vector<") : -1]
                lines.append(f"  {{")
                lines.append(f"    JsonArray arr = dest[{key}].to<JsonArray>();")
                lines.append(f"    for (const auto& item : value.{pname}) {{")
                if inner in ("double", "std::int64_t", "bool", "String"):
                    lines.append("      arr.add(item);")
                else:
                    lines.append("      JsonObject obj = arr.add<JsonObject>();")
                    lines.append("      if (!toJson(obj, item, err)) return false;")
                lines.append("    }")
                lines.append("  }")
            elif ctype in ("String", "std::int64_t", "bool", "double"):
                lines.append(f"  dest[{key}] = value.{pname};")
            else:
                lines.append(f"  if (!toJson(dest[{key}], value.{pname}, err)) return false;")
    lines.append("  return true;")
    lines.append("}")
    lines.append("")

    lines.append(
        f"inline bool fromJson(JsonObjectConst src, {name}& value, String& err) {{"
    )
    for pname, ctype, optional in fields:
        key = json.dumps(pname)
        if optional:
            lines.append(f"  if (src[{key}].isNull()) {{")
            lines.append(f"    value.has_{pname} = false;")
            lines.append("  } else {")
            lines.append(f"    value.has_{pname} = true;")
            if ctype == "JsonDocument":
                lines.append(f"    value.{pname}.clear();")
                lines.append(f"    if (!value.{pname}.set(src[{key}])) {{")
                lines.append(f'      err = "decode {pname}";')
                lines.append("      return false;")
                lines.append("    }")
            elif ctype.startswith("std::vector<"):
                inner = ctype[len("std::vector<") : -1]
                lines.append(f"    JsonArrayConst arr = src[{key}].as<JsonArrayConst>();")
                lines.append("    if (arr.isNull()) { err = \"decode " + pname + "\"; return false; }")
                lines.append(f"    value.{pname}.clear();")
                lines.append("    for (JsonVariantConst item : arr) {")
                if inner == "double":
                    lines.append(f"      value.{pname}.push_back(item.as<double>());")
                elif inner == "std::int64_t":
                    lines.append(f"      value.{pname}.push_back(item.as<std::int64_t>());")
                elif inner == "String":
                    lines.append(f"      value.{pname}.push_back(item.as<String>());")
                else:
                    lines.append(f"      {inner} elem{{}};")
                    lines.append(
                        "      if (!item.is<JsonObjectConst>() || !fromJson(item.as<JsonObjectConst>(), elem, err)) return false;"
                    )
                    lines.append(f"      value.{pname}.push_back(std::move(elem));")
                lines.append("    }")
            elif ctype == "String":
                lines.append(f"    value.{pname} = src[{key}].as<String>();")
            elif ctype == "std::int64_t":
                lines.append(f"    value.{pname} = src[{key}].as<std::int64_t>();")
            elif ctype == "bool":
                lines.append(f"    value.{pname} = src[{key}].as<bool>();")
            elif ctype == "double":
                lines.append(f"    value.{pname} = src[{key}].as<double>();")
            else:
                lines.append(
                    f"    if (!fromJson(src[{key}], value.{pname}, err)) return false;"
                )
            lines.append("  }")
        else:
            if ctype == "JsonDocument":
                lines.append(f"  if (!src[{key}].is<JsonObjectConst>() && !src[{key}].is<JsonArrayConst>()) {{")
                lines.append(f'    err = "missing {pname}";')
                lines.append("    return false;")
                lines.append("  }")
                lines.append(f"  value.{pname}.clear();")
                lines.append(f"  if (!value.{pname}.set(src[{key}])) {{")
                lines.append(f'    err = "decode {pname}";')
                lines.append("    return false;")
                lines.append("  }")
            elif ctype.startswith("std::vector<"):
                inner = ctype[len("std::vector<") : -1]
                lines.append(f"  {{")
                lines.append(f"    JsonArrayConst arr = src[{key}].as<JsonArrayConst>();")
                lines.append(
                    "    if (arr.isNull()) { err = \"missing "
                    + pname
                    + "\"; return false; }"
                )
                lines.append(f"    value.{pname}.clear();")
                lines.append("    for (JsonVariantConst item : arr) {")
                if inner == "double":
                    lines.append(f"      value.{pname}.push_back(item.as<double>());")
                else:
                    lines.append(f"      {inner} elem{{}};")
                    lines.append(
                        "      if (!item.is<JsonObjectConst>() || !fromJson(item.as<JsonObjectConst>(), elem, err)) return false;"
                    )
                    lines.append(f"      value.{pname}.push_back(std::move(elem));")
                lines.append("    }")
                lines.append("  }")
            elif ctype == "String":
                lines.append(f"  if (src[{key}].isNull()) {{ err = \"missing {pname}\"; return false; }}")
                lines.append(f"  value.{pname} = src[{key}].as<String>();")
            elif ctype == "std::int64_t":
                lines.append(f"  if (!src[{key}].is<std::int64_t>() && !src[{key}].is<int>()) {{ err = \"missing {pname}\"; return false; }}")
                lines.append(f"  value.{pname} = src[{key}].as<std::int64_t>();")
            elif ctype == "bool":
                lines.append(f"  if (!src[{key}].is<bool>()) {{ err = \"missing {pname}\"; return false; }}")
                lines.append(f"  value.{pname} = src[{key}].as<bool>();")
            elif ctype == "double":
                lines.append(f"  if (!src[{key}].is<double>() && !src[{key}].is<int>()) {{ err = \"missing {pname}\"; return false; }}")
                lines.append(f"  value.{pname} = src[{key}].as<double>();")
            else:
                # enum or nested struct via JsonVariant
                lines.append(
                    f"  if (!fromJson(src[{key}], value.{pname}, err)) return false;"
                )
    lines.append("  return true;")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def generate_embedded_payloads() -> None:
    """ArduinoJson payload types — no schema catalog, bool+String err codecs."""
    common = load(COMMON_NAME)["$defs"]

    chunks: list[str] = [
        "// @generated by scripts/generate_payloads.py — do not edit by hand.",
        "// Source of truth: schema/payloads/*.schema.json",
        "#pragma once",
        "",
        "#include <Arduino.h>",
        "#include <ArduinoJson.h>",
        "#include <cstdint>",
        "#include <cstring>",
        "#include <utility>",
        "#include <vector>",
        "",
        "namespace airbus {",
        "",
        "using PingResult = String;",
        "using AddResult = double;",
        "using AddParams = std::vector<double>;",
        "",
        "inline bool toJson(JsonVariant dest, const AddParams& value, String& /*err*/) {",
        "  JsonArray arr = dest.to<JsonArray>();",
        "  for (double item : value) arr.add(item);",
        "  return true;",
        "}",
        "",
        "inline bool fromJson(JsonVariantConst src, AddParams& value, String& err) {",
        "  JsonArrayConst arr = src.as<JsonArrayConst>();",
        "  if (arr.isNull()) { err = \"invalid AddParams\"; return false; }",
        "  value.clear();",
        "  for (JsonVariantConst item : arr) value.push_back(item.as<double>());",
        "  return true;",
        "}",
        "",
        "inline bool toJson(JsonVariant dest, const PingResult& value, String& /*err*/) {",
        "  dest.set(value);",
        "  return true;",
        "}",
        "",
        "inline bool fromJson(JsonVariantConst src, PingResult& value, String& err) {",
        "  if (src.isNull()) { err = \"invalid PingResult\"; return false; }",
        "  value = src.as<String>();",
        "  return true;",
        "}",
        "",
        "inline bool toJson(JsonVariant dest, const String& value, String& /*err*/) {",
        "  dest.set(value);",
        "  return true;",
        "}",
        "",
        "inline bool fromJson(JsonVariantConst src, String& value, String& err) {",
        "  if (src.isNull()) { err = \"invalid string\"; return false; }",
        "  value = src.as<String>();",
        "  return true;",
        "}",
        "",
        "inline bool toJson(JsonVariant dest, std::int64_t value, String& /*err*/) {",
        "  dest.set(value);",
        "  return true;",
        "}",
        "",
        "inline bool fromJson(JsonVariantConst src, std::int64_t& value, String& err) {",
        "  if (!src.is<std::int64_t>() && !src.is<int>()) { err = \"invalid int\"; return false; }",
        "  value = src.as<std::int64_t>();",
        "  return true;",
        "}",
        "",
        "inline bool toJson(JsonVariant dest, bool value, String& /*err*/) {",
        "  dest.set(value);",
        "  return true;",
        "}",
        "",
        "inline bool fromJson(JsonVariantConst src, bool& value, String& err) {",
        "  if (!src.is<bool>()) { err = \"invalid bool\"; return false; }",
        "  value = src.as<bool>();",
        "  return true;",
        "}",
        "",
        "inline bool toJson(JsonVariant dest, double value, String& /*err*/) {",
        "  dest.set(value);",
        "  return true;",
        "}",
        "",
        "inline bool fromJson(JsonVariantConst src, double& value, String& err) {",
        "  if (!src.is<double>() && !src.is<int>()) { err = \"invalid number\"; return false; }",
        "  value = src.as<double>();",
        "  return true;",
        "}",
        "",
        _emb_enum("QueueMode", list(common["QueueMode"]["enum"])),
        _emb_enum("DispatchStrategy", list(common["DispatchStrategy"]["enum"])),
        _emb_enum("DuplexSide", list(common["DuplexSide"]["enum"])),
        _emb_enum("Status", ["ok"]),
        _emb_struct(
            "QueueInfo",
            [
                ("name", "String", False),
                ("depth", "std::int64_t", False),
                ("mode", "QueueMode", False),
                ("listener_count", "std::int64_t", False),
            ],
        ),
        _emb_struct(
            "Event",
            [
                ("id", "String", False),
                ("event", "JsonDocument", False),
            ],
        ),
        _emb_struct(
            "Listener",
            [
                ("id", "String", False),
                ("queue", "String", False),
                ("host", "String", False),
                ("port", "std::int64_t", False),
                ("mode", "QueueMode", False),
                ("failure_count", "std::int64_t", False),
                ("active", "bool", False),
                ("side", "DuplexSide", True),
            ],
        ),
        _emb_struct(
            "PostEventParams",
            [
                ("queue", "String", False),
                ("event", "JsonDocument", False),
                ("side", "DuplexSide", True),
            ],
        ),
        _emb_struct(
            "PostEventResult",
            [
                ("id", "String", False),
                ("queue", "String", False),
            ],
        ),
        _emb_struct(
            "ListQueuesResult",
            [("queues", "std::vector<QueueInfo>", False)],
        ),
        _emb_struct(
            "PeekEventsParams",
            [
                ("queue", "String", False),
                ("count", "std::int64_t", True),
            ],
        ),
        _emb_struct(
            "PeekEventsResult",
            [
                ("queue", "String", False),
                ("events", "std::vector<Event>", False),
            ],
        ),
        _emb_struct(
            "CreateQueueParams",
            [
                ("queue", "String", False),
                ("mode", "QueueMode", True),
                ("dispatch_strategy", "DispatchStrategy", True),
            ],
        ),
        _emb_struct(
            "CreateQueueResult",
            [
                ("queue", "String", False),
                ("mode", "QueueMode", False),
                ("created", "bool", False),
            ],
        ),
        _emb_struct(
            "AttachListenerParams",
            [
                ("queue", "String", False),
                ("port", "std::int64_t", False),
                ("host", "String", True),
                ("exhaustion_timeout_ms", "std::int64_t", True),
                ("max_retries", "std::int64_t", True),
                ("side", "DuplexSide", True),
            ],
        ),
        _emb_struct(
            "AttachListenerResult",
            [
                ("listener_id", "String", False),
                ("queue", "String", False),
                ("status", "String", False),
            ],
        ),
        _emb_struct(
            "DetachListenerParams",
            [("listener_id", "String", False)],
        ),
        _emb_struct(
            "DetachListenerResult",
            [
                ("listener_id", "String", False),
                ("detached", "bool", False),
            ],
        ),
        _emb_struct(
            "ListListenersParams",
            [("queue", "String", True)],
        ),
        _emb_struct(
            "ListListenersResult",
            [("listeners", "std::vector<Listener>", False)],
        ),
        _emb_struct(
            "QueueReadyParams",
            [("queue", "String", False)],
        ),
        _emb_struct(
            "QueueReadyResult",
            [
                ("queue", "String", False),
                ("ready", "bool", False),
            ],
        ),
        _emb_struct(
            "ListenerEventParams",
            [
                ("queue", "String", False),
                ("id", "String", False),
                ("event", "JsonDocument", False),
            ],
        ),
        _emb_struct(
            "ListenerEventResult",
            [("status", "Status", False)],
        ),
        "}  // namespace airbus",
        "",
    ]

    _ = PAYLOAD_FILES
    EMBEDDED_PAYLOADS_OUT.parent.mkdir(parents=True, exist_ok=True)
    EMBEDDED_PAYLOADS_OUT.write_text("\n".join(chunks))
    try:
        shown = EMBEDDED_PAYLOADS_OUT.relative_to(ROOT)
    except ValueError:
        shown = EMBEDDED_PAYLOADS_OUT
    print(f"wrote {shown}", flush=True)


def generate_all_cpp() -> None:
    generate_embedded_payloads()
