"""Generate embedded ArduinoJson payload types from schema/payloads/*.schema.json."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

ROOT: Path
PAYLOADS: Path
COMMON_NAME: str
PAYLOAD_FILES: list[str]
EMBEDDED_PAYLOADS_OUT: Path

load: Callable[[str], dict]


def _cpp_enum_variant(value: str) -> str:
    parts = value.replace("-", "_").split("_")
    return "".join(p[:1].upper() + p[1:] for p in parts if p)


def _pascal(name: str) -> str:
    parts = name.replace("-", "_").split("_")
    return "".join(p[:1].upper() + p[1:] for p in parts if p)


def _singularize(name: str) -> str:
    if name.endswith("ies") and len(name) > 3:
        return name[:-3] + "y"
    if name.endswith("s") and not name.endswith("ss") and len(name) > 1:
        return name[:-1]
    return name


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
            elif ctype in ("String", "std::int64_t", "bool", "double"):
                # MemberProxy operator= — do NOT toJson(dest[key], …) then
                # JsonVariant::set(); that drops the field on ArduinoJson 7.
                lines.append(f"    dest[{key}] = value.{pname};")
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
                lines.append(
                    f"  if (!fromJson(src[{key}], value.{pname}, err)) return false;"
                )
    lines.append("  return true;")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def _emb_array_alias(name: str, inner: str) -> str:
    lines = [
        f"using {name} = std::vector<{inner}>;",
        "",
        f"inline bool toJson(JsonVariant dest, const {name}& value, String& /*err*/) {{",
        "  JsonArray arr = dest.to<JsonArray>();",
    ]
    if inner == "double":
        lines.append("  for (double item : value) arr.add(item);")
    elif inner == "std::int64_t":
        lines.append("  for (std::int64_t item : value) arr.add(item);")
    elif inner == "String":
        lines.append("  for (const String& item : value) arr.add(item);")
    else:
        raise SystemExit(f"unsupported array alias inner type: {inner}")
    lines.extend(
        [
            "  return true;",
            "}",
            "",
            f"inline bool fromJson(JsonVariantConst src, {name}& value, String& err) {{",
            "  JsonArrayConst arr = src.as<JsonArrayConst>();",
            f'  if (arr.isNull()) {{ err = "invalid {name}"; return false; }}',
            "  value.clear();",
        ]
    )
    if inner == "double":
        lines.append("  for (JsonVariantConst item : arr) value.push_back(item.as<double>());")
    elif inner == "std::int64_t":
        lines.append(
            "  for (JsonVariantConst item : arr) value.push_back(item.as<std::int64_t>());"
        )
    elif inner == "String":
        lines.append("  for (JsonVariantConst item : arr) value.push_back(item.as<String>());")
    lines.extend(["  return true;", "}", ""])
    return "\n".join(lines)


_PRIMITIVE_CODECS = """\
inline bool toJson(JsonVariant dest, const String& value, String& /*err*/) {
  dest.set(value);
  return true;
}

inline bool fromJson(JsonVariantConst src, String& value, String& err) {
  if (src.isNull()) { err = "invalid string"; return false; }
  value = src.as<String>();
  return true;
}

inline bool toJson(JsonVariant dest, std::int64_t value, String& /*err*/) {
  dest.set(value);
  return true;
}

inline bool fromJson(JsonVariantConst src, std::int64_t& value, String& err) {
  if (!src.is<std::int64_t>() && !src.is<int>()) { err = "invalid int"; return false; }
  value = src.as<std::int64_t>();
  return true;
}

inline bool toJson(JsonVariant dest, bool value, String& /*err*/) {
  dest.set(value);
  return true;
}

inline bool fromJson(JsonVariantConst src, bool& value, String& err) {
  if (!src.is<bool>()) { err = "invalid bool"; return false; }
  value = src.as<bool>();
  return true;
}

inline bool toJson(JsonVariant dest, double value, String& /*err*/) {
  dest.set(value);
  return true;
}

inline bool fromJson(JsonVariantConst src, double& value, String& err) {
  if (!src.is<double>() && !src.is<int>()) { err = "invalid number"; return false; }
  value = src.as<double>();
  return true;
}
"""


@dataclass
class EnumType:
    name: str
    values: list[str]


@dataclass
class StructType:
    name: str
    fields: list[tuple[str, str, bool]]  # json_name, cpp_type, optional
    deps: set[str] = field(default_factory=set)


@dataclass
class AliasType:
    name: str
    cpp_type: str


@dataclass
class ArrayAliasType:
    name: str
    inner: str


class SchemaCppEmitter:
    """Walk JSON Schema payload defs and emit ArduinoJson C++ types."""

    def __init__(self, common_defs: dict) -> None:
        self.common_defs = common_defs
        self.enums: dict[str, EnumType] = {}
        self.structs: dict[str, StructType] = {}
        self.aliases: dict[str, AliasType] = {}
        self.array_aliases: dict[str, ArrayAliasType] = {}
        # $defs keys that collapse to builtins (no named C++ type)
        self.ref_builtins: dict[str, str] = {}

    def _is_string_enum(self, schema: dict) -> bool:
        return schema.get("type") == "string" and "enum" in schema

    def _is_freeform_object(self, schema: dict) -> bool:
        if schema.get("type") != "object":
            return False
        if schema.get("properties"):
            return False
        return schema.get("additionalProperties", False) is not False

    def _is_object(self, schema: dict) -> bool:
        return schema.get("type") == "object" and bool(schema.get("properties"))

    def _ref_key(self, ref: str) -> str | None:
        for prefix in (f"{COMMON_NAME}#/$defs/", "#/$defs/"):
            if ref.startswith(prefix):
                return ref[len(prefix) :]
        return None

    def register_common(self) -> None:
        for key, schema in self.common_defs.items():
            if not isinstance(schema, dict):
                continue
            if self._is_string_enum(schema):
                self._ensure_enum(key, list(schema["enum"]))
            elif self._is_freeform_object(schema):
                self.ref_builtins[key] = "JsonDocument"
            elif schema.get("type") == "string":
                self.ref_builtins[key] = "String"
            elif self._is_object(schema):
                self._ensure_struct(key, schema)

    def register_payload(self, title: str, schema: dict) -> None:
        typ = schema.get("type")
        if typ == "object":
            self._ensure_struct(title, schema)
            return
        if typ == "array":
            items = schema.get("items") or {}
            inner = self._resolve_type(items, hint=f"{title}Item")
            self.array_aliases[title] = ArrayAliasType(title, inner)
            return
        if typ == "string":
            if self._is_string_enum(schema):
                self._ensure_enum(title, list(schema["enum"]))
            else:
                self.aliases[title] = AliasType(title, "String")
            return
        if typ == "number":
            self.aliases[title] = AliasType(title, "double")
            return
        if typ == "integer":
            self.aliases[title] = AliasType(title, "std::int64_t")
            return
        if typ == "boolean":
            self.aliases[title] = AliasType(title, "bool")
            return
        raise SystemExit(f"unsupported root schema type for {title}: {typ!r}")

    def _ensure_enum(self, name: str, values: list[str]) -> str:
        existing = self.enums.get(name)
        if existing is not None:
            if existing.values != values:
                raise SystemExit(
                    f"conflicting enum {name}: {existing.values} vs {values}"
                )
            return name
        self.enums[name] = EnumType(name, values)
        return name

    def _ensure_struct(self, name: str, schema: dict) -> str:
        existing = self.structs.get(name)
        if existing is not None:
            return name
        # Placeholder to break cycles while resolving fields
        self.structs[name] = StructType(name, [], set())
        required = set(schema.get("required") or [])
        props = schema.get("properties") or {}
        fields: list[tuple[str, str, bool]] = []
        deps: set[str] = set()
        for pname, prop_schema in props.items():
            if not isinstance(prop_schema, dict):
                raise SystemExit(f"{name}.{pname}: expected object schema")
            ctype = self._resolve_type(prop_schema, hint=_pascal(pname), parent_prop=pname)
            optional = pname not in required
            fields.append((pname, ctype, optional))
            base = ctype[len("std::vector<") : -1] if ctype.startswith("std::vector<") else ctype
            if base in self.structs or base in self.enums:
                deps.add(base)
        self.structs[name] = StructType(name, fields, deps)
        return name

    def _resolve_type(
        self, schema: dict, *, hint: str, parent_prop: str | None = None
    ) -> str:
        if "$ref" in schema:
            key = self._ref_key(schema["$ref"])
            if key is None:
                raise SystemExit(f"unsupported $ref: {schema['$ref']}")
            if key in self.ref_builtins:
                return self.ref_builtins[key]
            if key in self.enums:
                return key
            if key in self.structs:
                return key
            target = self.common_defs.get(key)
            if not isinstance(target, dict):
                raise SystemExit(f"unknown $ref target: {key}")
            if self._is_string_enum(target):
                return self._ensure_enum(key, list(target["enum"]))
            if self._is_object(target):
                return self._ensure_struct(key, target)
            if self._is_freeform_object(target):
                return "JsonDocument"
            if target.get("type") == "string":
                return "String"
            raise SystemExit(f"unsupported common $def {key}")

        if self._is_string_enum(schema):
            name = hint if hint[0].isupper() else _pascal(parent_prop or hint)
            return self._ensure_enum(name, list(schema["enum"]))

        typ = schema.get("type")
        if typ == "string":
            return "String"
        if typ == "integer":
            return "std::int64_t"
        if typ == "number":
            return "double"
        if typ == "boolean":
            return "bool"
        if typ == "array":
            items = schema.get("items") or {}
            if not isinstance(items, dict):
                raise SystemExit("array items must be a schema object")
            item_hint = _pascal(_singularize(parent_prop or hint))
            inner = self._resolve_type(items, hint=item_hint, parent_prop=parent_prop)
            return f"std::vector<{inner}>"
        if self._is_freeform_object(schema):
            return "JsonDocument"
        if self._is_object(schema):
            name = schema.get("title") or hint
            if not isinstance(name, str) or not name:
                raise SystemExit(f"object schema needs a title or hint: {schema}")
            return self._ensure_struct(name, schema)

        raise SystemExit(f"unsupported schema: {schema}")

    def _struct_emit_order(self) -> list[StructType]:
        pending = dict(self.structs)
        ordered: list[StructType] = []
        seen: set[str] = set()

        def visit(name: str) -> None:
            if name in seen:
                return
            st = pending.get(name)
            if st is None:
                return
            seen.add(name)
            for dep in sorted(st.deps):
                if dep in pending:
                    visit(dep)
            ordered.append(st)

        for name in list(pending):
            visit(name)
        return ordered

    def render(self) -> str:
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
        ]

        for alias in self.aliases.values():
            chunks.append(f"using {alias.name} = {alias.cpp_type};")
        if self.aliases:
            chunks.append("")

        for arr in self.array_aliases.values():
            chunks.append(_emb_array_alias(arr.name, arr.inner))

        # Primitive codecs after aliases that are typedefs of String/double so we
        # do not emit redefining overloads for those aliases.
        chunks.append(_PRIMITIVE_CODECS.rstrip("\n"))
        chunks.append("")

        for enum in self.enums.values():
            chunks.append(_emb_enum(enum.name, enum.values))
        for st in self._struct_emit_order():
            chunks.append(_emb_struct(st.name, st.fields))

        chunks.append("}  // namespace airbus")
        chunks.append("")
        return "\n".join(chunks)


def generate_embedded_payloads() -> None:
    """ArduinoJson payload types — inferred from schema/payloads/*.schema.json."""
    common = load(COMMON_NAME)["$defs"]
    emitter = SchemaCppEmitter(common)
    emitter.register_common()

    for fname in PAYLOAD_FILES:
        doc = load(fname)
        title = doc["title"]
        body = {k: v for k, v in doc.items() if k not in ("$schema", "$id", "title")}
        emitter.register_payload(title, body)

    EMBEDDED_PAYLOADS_OUT.parent.mkdir(parents=True, exist_ok=True)
    EMBEDDED_PAYLOADS_OUT.write_text(emitter.render())
    try:
        shown = EMBEDDED_PAYLOADS_OUT.relative_to(ROOT)
    except ValueError:
        shown = EMBEDDED_PAYLOADS_OUT
    print(f"wrote {shown}", flush=True)


def generate_all_cpp() -> None:
    generate_embedded_payloads()
