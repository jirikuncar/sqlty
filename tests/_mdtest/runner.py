"""Evaluate a section's assertions through ``sqlty.core``."""

from __future__ import annotations

import difflib
import re
from collections.abc import Iterator
from contextlib import contextmanager
from functools import cached_property
from importlib import import_module

import pytest

import sqlty.core as core

from .parser import Assertion, Kind, Section
from .updater import Updater

BACKENDS = {"py": "sqlty._core_py", "rs": "sqlty._core"}
_ERROR_VALUE = re.compile(r'^(?:\[(?P<rule>[\w-]+)\])?\s*(?:"(?P<message>.*)")?$')


class MDTestFailure(Exception):
    pass


@contextmanager
def backend(name: str) -> Iterator[None]:
    """Route ``sqlty.core`` to one backend."""
    module = import_module(BACKENDS[name])
    with pytest.MonkeyPatch.context() as mp:
        for attr in ("check_source", "rewrite_source", "strip_source"):
            mp.setattr(core, f"_backend_{attr}", getattr(module, attr))
        for attr in ("infer_type", "expand_star"):
            mp.setattr(core, attr, getattr(module, attr))
        yield


class Run:
    def __init__(self, section: Section, updater: Updater | None) -> None:
        self.section = section
        self.updater = updater
        settings = section.settings
        self.dialect: str = settings.get("dialect", "postgres")
        self.schema_sql: str | None = section.files.get(settings.get("schema", ""))
        self.import_style: str | None = settings.get("import-style")
        self.untyped: bool = settings.get("untyped") == "sentinel"
        self.rules: dict[str, str] | None = settings.get("rules")

    def run(self) -> None:
        failures: list[str] = []
        errors = [a for a in self.section.assertions if a.kind == "error"]
        for assertion in self.section.assertions:
            if assertion.kind == "error":
                continue
            actual = self.compute(assertion.kind)
            if actual == assertion.expected:
                continue
            if self.updater:
                self.updater.replace(assertion, actual.split("\n"))
            else:
                failures.append(self._mismatch(assertion, actual))
        if errors:
            failures.extend(self._check(errors))
        if failures:
            raise MDTestFailure("\n\n".join(failures))

    def compute(self, kind: Kind) -> str:
        code = self.section.code.text
        match kind:
            case "inferred":
                query = self.section.query
                return str(core.infer_type(query, dialect=self.dialect, schema_sql=self.schema_sql))
            case "expanded":
                query = self.section.query
                return str(
                    core.expand_star(query, dialect=self.dialect, schema_sql=self.schema_sql)
                )
            case "rewritten":
                return _without_imports(self._rewritten, self._imports).strip()
            case "rewritten-file":
                return self._rewritten.strip()
            case "stripped":
                return core.strip_source(code, import_style=self.import_style).strip()
            case "decoded":
                from sqlty._decode import decode

                return _without_imports(decode(code.encode())[0], self._imports).strip()
            case "error":
                raise AssertionError("errors are matched per line")

    @cached_property
    def _rewritten(self) -> str:
        return core.rewrite_source(
            self.section.code.text,
            dialect=self.dialect,
            schema_sql=self.schema_sql,
            import_style=self.import_style,
            untyped=self.untyped,
        )

    @cached_property
    def _imports(self) -> set[str]:
        from sqlty._rewriter import ImportStyle

        style = ImportStyle.parse(self.import_style) if self.import_style else ImportStyle.default()
        extra = {style.untyped_import} if style.untyped_import else set()
        return {"import datetime", "import uuid", style.import_line, *extra}

    def _check(self, assertions: list[Assertion]) -> list[str]:
        code = self.section.code
        diagnostics = [
            (code.md_lines[int(str(d["line"])) - 1], str(d["rule"]), str(d["message"]))
            for d in core.check_source(
                code.text,
                dialect=self.dialect,
                schema_sql=self.schema_sql,
                rules=self.rules,
                import_style=self.import_style,
            )
        ]
        expected = [a for a in assertions if a.expected]
        unmatched = list(expected)
        unexpected = []
        for diagnostic in diagnostics:
            match = next((a for a in unmatched if _matches(a, diagnostic)), None)
            if match is None:
                unexpected.append(diagnostic)
            else:
                unmatched.remove(match)
        if not (unmatched or unexpected):
            return []
        if self.updater:
            self.updater.replace_errors(assertions, diagnostics)
            return []
        path = self.section.path
        return [
            *(f"{path}:{a.target}: unmatched assertion `# error: {a.expected}`" for a in unmatched),
            *(
                f'{path}:{line}: unexpected diagnostic [{rule}] "{msg}"'
                for line, rule, msg in unexpected
            ),
        ]

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


def _matches(assertion: Assertion, diagnostic: tuple[int, str, str]) -> bool:
    line, rule, message = diagnostic
    value = _ERROR_VALUE.match(assertion.expected)
    if value is None or assertion.target != line:
        return False
    return (value["rule"] in (None, rule)) and (
        value["message"] is None or value["message"] in message
    )


def _without_imports(source: str, imports: set[str]) -> str:
    return "\n".join(line for line in source.splitlines() if line.rstrip() not in imports)
