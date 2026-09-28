#!/usr/bin/env python3
"""Keep wire-envelope construction/inspection inside explicit protocol modules."""

from __future__ import annotations

import ast
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ENVELOPE = {"jsonrpc", "method", "params", "result", "error"}


def without_rust_tests(source: str) -> str:
    """Exclude explicit test modules while retaining any production code after them."""
    token = re.compile(
        r'r(?P<hashes>#+)".*?"(?P=hashes)|"(?:\\.|[^"\\])*"|//[^\n]*|/\*.*?\*/|[{}]', re.S
    )
    pattern = re.compile(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{")
    while match := pattern.search(source):
        depth = 1
        for item in token.finditer(source, match.end()):
            if item.group() == "{":
                depth += 1
            elif item.group() == "}":
                depth -= 1
            if depth == 0:
                source = (
                    source[: match.start()]
                    + "\n" * source.count("\n", match.start(), item.end())
                    + source[item.end() :]
                )
                break
        else:
            raise ValueError("unclosed Rust test module")
    return source


def violations(path: Path, source: str) -> list[str]:
    issues: list[str] = []
    if path.suffix == ".py":
        for node in ast.walk(ast.parse(source)):
            keys: list[ast.AST] = []
            if isinstance(node, ast.Dict):
                keys = [key for key in node.keys if key is not None]
            elif isinstance(node, ast.Subscript):
                keys = [node.slice]
            elif isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute):
                if node.func.attr == "get":
                    keys = node.args[:1]
            elif (
                isinstance(node, ast.Call)
                and isinstance(node.func, ast.Name)
                and node.func.id == "dict"
            ):
                keys = [ast.Constant(value=keyword.arg) for keyword in node.keywords]
            elif isinstance(node, ast.Compare):
                keys = [node.left]
            if any(
                isinstance(key, ast.Constant)
                and isinstance(key.value, str)
                and key.value in ENVELOPE
                for key in keys
            ):
                issues.append(f"{path}:{node.lineno}: wire fields belong in protocol.py")
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and (
                node.name.endswith("_wire_params") or node.name.startswith("_from_")
            ):
                issues.append(f"{path}:{node.lineno}: use the shared generated-payload codec")
    else:
        if path.suffix == ".rs":
            source = without_rust_tests(source)
        # Deliberately narrow lexical guard for Rust/JS: protocol keys, not arbitrary JSON data.
        # Preserve strings containing URLs; their // must not hide subsequent code.
        source = re.sub(
            r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|/\*.*?\*/|//[^\n]*',
            lambda match: (
                "\n" * match.group().count("\n")
                if match.group().startswith(("//", "/*"))
                else match.group()
            ),
            source,
            flags=re.S,
        )
        patterns = [
            r'["\']jsonrpc["\']\s*:',
            r"\bjsonrpc\s*:",
            r'\.(?:get|insert)\(\s*"(?:jsonrpc|params|result|error)"',
            r'\[\s*"(?:jsonrpc|params|result|error)"\s*\]',
        ]
        if path.suffix == ".js":
            patterns.append(r"\bresponse\.(?:jsonrpc|result|error)\b")
        for pattern in patterns:
            for match in re.finditer(pattern, source):
                line = source.count("\n", 0, match.start()) + 1
                issues.append(f"{path}:{line}: wire fields belong in the protocol module")
    return issues


def check(root: Path = ROOT) -> list[str]:
    paths = [
        *root.joinpath("src").rglob("*.rs"),
        *root.joinpath("client-py/src").rglob("*.py"),
        *root.joinpath("resources/ui").rglob("*.js"),
    ]
    allowed = {
        "client-py/src/airbus_client/protocol.py",
        "client-py/src/airbus_client/payloads.py",
        "resources/ui/protocol.js",
    }
    issues = []
    for path in paths:
        relative = path.relative_to(root)
        if relative.as_posix().startswith("src/proto/") or relative.as_posix() in allowed:
            continue
        issues.extend(violations(relative, path.read_text()))
    return issues


if __name__ == "__main__":
    errors = check()
    print("\n".join(errors) if errors else "Protocol boundaries kept.")
    raise SystemExit(bool(errors))
