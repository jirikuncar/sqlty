"""Evaluate the `# decoded:` assertions of a section through the magic codec."""

from __future__ import annotations

import ast
import difflib
import io
import re
import tokenize
from functools import cached_property

from .parser import Assertion, Kind, Section, Unit
from .updater import Updater

_CODING = re.compile(r"\A#.*coding[:=].*\n?")
PYTHON_KINDS = frozenset({"decoded"})


class MDTestFailure(Exception):
    pass


class Run:
    """The assertions only Python can check: ``# decoded:``, through the magic codec.

    The Rust runners (``rust/mdtest``) check every other kind, each in the crate it covers.
    """

    def __init__(self, section: Section, updater: Updater | None) -> None:
        self.section = section
        self.updater = updater
        self.import_style: str | None = section.settings.get("import-style")

    def run(self) -> None:
        failures: list[str] = []
        for unit in self.section.units:
            for assertion in unit.assertions:
                if assertion.kind not in PYTHON_KINDS:
                    continue
                actual = self.compute(assertion.kind, unit)
                if actual == assertion.expected:
                    continue
                if self.updater:
                    self.updater.replace(assertion, actual.split("\n"))
                else:
                    failures.append(self._mismatch(assertion, actual))
        if failures:
            raise MDTestFailure("\n\n".join(failures))

    def compute(self, kind: Kind, unit: Unit) -> str:
        from sqlty.magic import decode

        decoded = _without_imports(decode(unit.code.text.encode())[0], self._imports).strip()
        return _CODING.sub("", decoded, count=1).strip()

    @cached_property
    def _imports(self) -> set[str]:
        style = (self.import_style or "from sqlty import SQL").strip()
        extra = (
            {"from sqlty import UNTYPED", "from sqlty import SQLTemplate"}
            if style.startswith("from ")
            else set()
        )
        return {"import datetime", "import typing", "import uuid", style, *extra}

    def _mismatch(self, assertion: Assertion, actual: str) -> str:
        head = (
            f"{self.section.path}:{assertion.line}: `{assertion.comment} {assertion.kind}:` failed"
        )
        if "\n" not in assertion.expected + actual:
            return f"{head}\n  expected: {assertion.expected}\n  actual:   {actual}"
        diff = difflib.unified_diff(
            assertion.expected.splitlines(), actual.splitlines(), "expected", "actual", lineterm=""
        )
        return head + "\n" + "\n".join(diff)


def owns(section: Section) -> bool:
    """Whether ``section`` holds an assertion this runner checks."""
    return any(a.kind in PYTHON_KINDS for unit in section.units for a in unit.assertions)


def string_constants(source: str) -> list[str]:
    """Every string value in ``source``, implicit concatenation applied, sorted.

    Strings inside the subscript of a called wrapper (``SQL[...](...)``) are left out:
    they are part of the row type, which rewrite writes (an enum column's
    ``typing.Literal[...]``) and strip removes (a forward reference, ``SQL["User"]``).
    """
    tree = ast.parse(source)
    labels = {
        id(node)
        for call in ast.walk(tree)
        if isinstance(call, ast.Call) and isinstance(call.func, ast.Subscript)
        for node in ast.walk(call.func.slice)
    }
    return sorted(
        node.value
        for node in ast.walk(tree)
        if isinstance(node, ast.Constant) and isinstance(node.value, str) and id(node) not in labels
    )


_GENERATED = re.compile(
    r"# sqlty: generated\r?\n(?:class [^\r\n]*:\r?\n(?:    [^\r\n]*\r?\n)+"
    r"|\w+ = typing\.TypedDict\([^\r\n]*\)\r?\n)"
)


def without_generated(source: str) -> str:
    """``source`` without the classes ``json-types = "typing.TypedDict"`` generates, which are
    new code with comments and strings of their own."""
    return _GENERATED.sub("", source)


def comments(source: str) -> list[str]:
    """Every comment in ``source``, sorted."""
    return sorted(
        token.string
        for token in tokenize.generate_tokens(io.StringIO(source).readline)
        if token.type == tokenize.COMMENT
    )


def _without_imports(source: str, imports: set[str]) -> str:
    lines = []
    for line in source.splitlines():
        head, sep, rest = line.partition("; ")
        while sep and head in imports:
            line = rest
            head, sep, rest = line.partition("; ")
        if line.rstrip() not in imports:
            lines.append(line)
    return "\n".join(lines)
