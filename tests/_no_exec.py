"""pytest plugin: fail a test in which sqlty executes, evaluates or imports what a source names.

An audit hook (PEP 578) sees every ``exec``/``eval`` and every import. One that sqlty code
runs, or that code it executed runs, is charged to the innermost function of ``src/sqlty``:
its ``import`` statements and module bodies are free, anything else fails the test unless
the function is listed in ``tests/no_exec_allowlist.txt``. Libraries evaluating on sqlty's
behalf (``typing.get_type_hints``) are out of the hook's sight; `sites` finds those calls
statically.

Audit hooks are no sandbox (code can bypass them); this catches mistakes, review catches
the rest.
"""

from __future__ import annotations

import ast
import functools
import importlib
import sys
import sysconfig
import types
from collections.abc import Iterator
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src" / "sqlty"
ALLOWLIST = Path(__file__).with_name("no_exec_allowlist.txt")

_CALLS = frozenset({"exec", "eval", "compile", "__import__", "breakpoint"})
_METHODS = frozenset(
    {
        "exec",
        "eval",
        "__import__",
        "import_module",
        "reload",
        "run_path",
        "run_module",
        "exec_module",
        "load_module",
        "get_type_hints",
        "evaluate_forward_ref",
        "get_annotations",
        "call_evaluate_function",
        "call_annotate_function",
        "new_class",
    }
)
_LIBRARIES = tuple(
    {sysconfig.get_path(key) for key in ("stdlib", "platstdlib", "purelib", "platlib")}
)
_IMPORT_MODULE = importlib.__file__

_violations: list[str] = []


@functools.cache
def allowed() -> frozenset[str]:
    lines = ALLOWLIST.read_text().splitlines()
    return frozenset(line for line in lines if line and not line.startswith("#"))


def _module(path: Path) -> str:
    parts = path.relative_to(SRC.parent).with_suffix("").parts
    return ".".join(parts[:-1] if parts[-1] == "__init__" else parts)


class _Sites(ast.NodeVisitor):
    def __init__(self, module: str) -> None:
        self.module = module
        self.scope: list[str] = []
        self.found: dict[str, list[int]] = {}

    def _enter(self, node: ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef) -> None:
        self.scope.append(node.name)
        self.scope.append("<locals>")
        self.generic_visit(node)
        del self.scope[-2:]

    visit_FunctionDef = visit_AsyncFunctionDef = _enter

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        self.scope.append(node.name)
        self.generic_visit(node)
        self.scope.pop()

    def visit_Call(self, node: ast.Call) -> None:
        func = node.func
        if (isinstance(func, ast.Name) and func.id in _CALLS) or (
            isinstance(func, ast.Attribute) and func.attr in _METHODS
        ):
            qualname = ".".join(self.scope).removesuffix(".<locals>") or "<module>"
            self.found.setdefault(f"{self.module}:{qualname}", []).append(node.lineno)
        self.generic_visit(node)


def sites() -> dict[str, list[str]]:
    """The calls of ``src/sqlty`` that can execute or evaluate code, by ``module:qualname``."""
    found: dict[str, list[str]] = {}
    for path in sorted(SRC.rglob("*.py")):
        visitor = _Sites(_module(path))
        visitor.visit(ast.parse(path.read_text(), str(path)))
        for site, lines in visitor.found.items():
            found.setdefault(site, []).extend(f"{path.relative_to(ROOT)}:{n}" for n in lines)
    return found


@functools.cache
def _import_lines(filename: str) -> frozenset[int] | None:
    """Lines of the ``import`` statements of a file of ``src/sqlty``; None for any other file."""
    path = Path(filename)
    if path.suffix != ".py" or not path.is_relative_to(SRC):
        return None
    return frozenset(
        line
        for node in ast.walk(ast.parse(path.read_text()))
        if isinstance(node, (ast.Import, ast.ImportFrom))
        for line in range(node.lineno, (node.end_lineno or node.lineno) + 1)
    )


def _charge(frame: types.FrameType | None, what: str) -> None:
    """Record ``what`` against the innermost sqlty function at or below ``frame``."""
    while frame is not None:
        code = frame.f_code
        if (lines := _import_lines(code.co_filename)) is not None:
            if code.co_name == "<module>" or frame.f_lineno in lines:
                return
            site = f"{_module(Path(code.co_filename))}:{code.co_qualname}"
            if site not in allowed():
                _violations.append(f"{site} ran {what}")
            return
        frame = frame.f_back


def _library(filename: str) -> bool:
    return _import_lines(filename) is None and filename.startswith(("<frozen", *_LIBRARIES))


def _hook(event: str, args: tuple[object, ...]) -> None:
    if event == "exec":
        caller = sys._getframe(1)
        if not _library(caller.f_code.co_filename):
            _charge(caller, f"exec of {getattr(args[0], 'co_filename', '?')}")
    elif event == "import":
        frame: types.FrameType | None = sys._getframe(1)
        while frame is not None and (
            frame.f_code.co_filename.startswith("<frozen importlib")
            or frame.f_code.co_filename == _IMPORT_MODULE
        ):
            frame = frame.f_back
        if frame is not None and not _library(frame.f_code.co_filename):
            _charge(frame, f"import {args[0]}")


def pytest_configure(config: pytest.Config) -> None:
    sys.addaudithook(_hook)


@pytest.hookimpl(wrapper=True)
def pytest_runtest_call(item: pytest.Item) -> Iterator[None]:
    _violations.clear()
    result = yield
    if found := list(dict.fromkeys(_violations)):
        _violations.clear()
        raise AssertionError(
            "sqlty executed code named by a checked source:\n  " + "\n  ".join(found)
        )
    return result
