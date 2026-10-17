"""Evaluate a section's assertions through ``sqlty.core``."""

from __future__ import annotations

import ast
import difflib
import io
import re
import tokenize
from functools import cached_property

import sqlty.core as core

from .parser import Assertion, Kind, Section, Unit
from .updater import Updater

_CODING = re.compile(r"\A#.*coding[:=].*\n?")
_ERROR_VALUE = re.compile(r'^(?:\[(?P<rule>[\w-]+)\])?\s*(?:"(?P<message>.*)")?$')


class MDTestFailure(Exception):
    pass


class Run:
    def __init__(self, section: Section, updater: Updater | None) -> None:
        self.section = section
        self.updater = updater
        settings = section.settings
        self.dialect: str = settings.get("dialect", "postgres")
        self.schema_sql: str | None = section.schema_sql
        self.import_style: str | None = settings.get("import-style")
        self.untyped: bool = settings.get("untyped") == "sentinel"
        self.json_types: str = settings.get("json-types", "typing.Any")
        self.rules: dict[str, str] | None = settings.get("rules")
        self.type_modules: list[str] | None = settings.get("type-modules")
        # With an `against.sql` file, `# error:` asserts what `sqlty breaking` reports.
        self.against_sql: str | None = section.files.get("against.sql")

    def run(self) -> None:
        failures: list[str] = []
        for unit in self.section.units:
            failures.extend(self._run_unit(unit))
        if failures:
            raise MDTestFailure("\n\n".join(failures))

    def _run_unit(self, unit: Unit) -> list[str]:
        failures: list[str] = []
        errors = [a for a in unit.assertions if a.kind == "error"]
        for assertion in unit.assertions:
            if assertion.kind == "error":
                continue
            actual = self.compute(assertion.kind, unit)
            if actual == assertion.expected:
                continue
            if self.updater:
                self.updater.replace(assertion, actual.split("\n"))
            else:
                failures.append(self._mismatch(assertion, actual))
        if errors:
            failures.extend(self._check(errors, unit))
        return failures

    def compute(self, kind: Kind, unit: Unit) -> str:
        text = unit.code.text
        match kind:
            case "inferred":
                return str(core.infer_type(text, dialect=self.dialect, schema_sql=self.schema_sql))
            case "expanded":
                return str(core.expand_star(text, dialect=self.dialect, schema_sql=self.schema_sql))
            case "columns":
                columns = core.analyze_select(
                    text, dialect=self.dialect, schema_sql=self.schema_sql
                )
                if columns is None:
                    return "None"
                return "\n".join(f"{c.name}: {c.python_type} ({c.sql_type})" for c in columns)
            case "json":
                shapes = core.json_shapes(text, dialect=self.dialect, schema_sql=self.schema_sql)
                return "\n".join(f"{name}: {shape}" for name, shape in shapes) or "None"
            case "params":
                params = core.analyze_params(text, dialect=self.dialect, schema_sql=self.schema_sql)
                if params is None:
                    return "None"
                nullable = {True: " NULL", False: " NOT NULL", None: ""}
                return "\n".join(
                    f"{p.name or p.number}: {p.python_type} ({p.sql_type}){nullable[p.nullable]}"
                    for p in params
                )
            case "rewritten":
                rewritten = _parses(text, self._rewrite(text), same_strings=True)
                return _without_imports(rewritten, self._imports).strip()
            case "rewritten-file":
                return _parses(text, self._rewrite(text), same_strings=True).strip()
            case "fixed":
                fixed = _parses(text, self._fix(text))
                if (again := self._fix(fixed)) != fixed:
                    raise AssertionError(
                        f"check --fix is not idempotent; a second run gives:\n{again}"
                    )
                return fixed.strip()
            case "stripped":
                stripped = core.strip_source(text, import_style=self.import_style)
                return _parses(text, stripped, same_strings=True).strip()
            case "decoded":
                from sqlty._decode import decode

                decoded = _without_imports(decode(text.encode())[0], self._imports).strip()
                return _CODING.sub("", decoded, count=1).strip()
            case "error":
                raise AssertionError("errors are matched per line")

    def _fix(self, text: str) -> str:
        return core.fix_source(
            text,
            dialect=self.dialect,
            schema_sql=self.schema_sql,
            import_style=self.import_style,
            untyped=self.untyped,
            rules=self.rules,
            json_types=self.json_types,
        )

    def _rewrite(self, text: str) -> str:
        return core.rewrite_source(
            text,
            dialect=self.dialect,
            schema_sql=self.schema_sql,
            import_style=self.import_style,
            untyped=self.untyped,
            json_types=self.json_types,
        )

    @cached_property
    def _imports(self) -> set[str]:
        style = (self.import_style or "from sqlty import SQL").strip()
        extra = (
            {"from sqlty import UNTYPED", "from sqlty import SQLTemplate"}
            if style.startswith("from ")
            else set()
        )
        return {"import datetime", "import typing", "import uuid", style, *extra}

    @cached_property
    def _helpers(self) -> core.HelperIndex | None:
        """Labelled ``.py`` files as modules (``pkg/db.py`` is ``pkg.db``), plus ``functions``."""
        modules = {
            path.removesuffix(".py").replace("/", "."): text
            for path, text in self.section.files.items()
            if path.endswith(".py")
        }
        functions = self.section.settings.get("functions", [])
        if not modules and not functions:
            return None
        index = core.HelperIndex()
        for module, text in modules.items():
            index.add(module.removesuffix(".__init__"), core.ParsedSource(text, module))
        for f in functions:
            index.register(f["import-path"], f["query-param"], f.get("return-type-param"))
        return index

    def _check(self, assertions: list[Assertion], unit: Unit) -> list[str]:
        code = unit.code
        if self.against_sql is not None:
            found = core.breaking_source(
                code.text,
                schema_sql=self.schema_sql,
                against_sql=self.against_sql,
                dialect=self.dialect,
                rules=self.rules,
                import_style=self.import_style,
                untyped=self.untyped,
                type_modules=self.type_modules,
                helpers=self._helpers,
            )
        else:
            found = core.check_source(
                code.text,
                dialect=self.dialect,
                schema_sql=self.schema_sql,
                rules=self.rules,
                import_style=self.import_style,
                untyped=self.untyped,
                type_modules=self.type_modules,
                helpers=self._helpers,
            )
        diagnostics = [
            (code.md_lines[int(str(d["line"])) - 1], str(d["rule"]), str(d["message"]))
            for d in found
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


def string_constants(source: str) -> list[str]:
    """Every string value in ``source``, implicit concatenation applied, sorted.

    The labels of a ``Literal[...]`` inside the subscript of a called wrapper
    (``SQL[...](...)``) are left out: rewrite writes an enum column's type as
    ``typing.Literal[...]`` into that annotation, and nowhere else.
    """
    tree = ast.parse(source)
    labels = {
        id(node)
        for call in ast.walk(tree)
        if isinstance(call, ast.Call) and isinstance(call.func, ast.Subscript)
        for sub in ast.walk(call.func.slice)
        if isinstance(sub, ast.Subscript)
        and ast.unparse(sub.value) in ("Literal", "typing.Literal")
        for node in ast.walk(sub.slice)
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


def _parses(source: str, output: str, *, same_strings: bool = False) -> str:
    """``output``, which must be valid Python with the same comments when ``source`` is.

    With ``same_strings``, it must also hold the same strings: rewrite and strip
    only add or remove wrappers, never join, split or change a string.
    """
    try:
        ast.parse(source)
    except SyntaxError:
        return output
    try:
        ast.parse(output)
    except SyntaxError as error:
        raise AssertionError(f"output is not valid Python ({error}):\n{output}") from error
    # Classes rewrite generates for JSON columns are new code, with comments and strings.
    written, original = without_generated(output), without_generated(source)
    if comments(written) != comments(original):
        raise AssertionError(f"output changes the comments of the source:\n{output}")
    if same_strings and string_constants(written) != string_constants(original):
        raise AssertionError(f"output changes the strings of the source:\n{output}")
    return output


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
