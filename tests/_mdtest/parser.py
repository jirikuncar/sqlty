"""Markdown -> test sections.

A heading opens a section; sections with assertions are tests. Setup
(``toml`` config and labelled files) is inherited by nested sections, code
never is.

Each code block with assertions is a unit: a ``sql`` block on its own, a
``python`` block together with the assertion-free python blocks right before
it (literate setup). Blocks after it are not part of it.
"""

from __future__ import annotations

import re
import tomllib
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal, cast

Kind = Literal[
    "inferred",
    "expanded",
    "columns",
    "rewritten",
    "rewritten-file",
    "stripped",
    "fixed",
    "error",
    "decoded",
]
SQL_KINDS: tuple[Kind, ...] = ("inferred", "expanded", "columns")
PY_KINDS: tuple[Kind, ...] = (
    "rewritten",
    "rewritten-file",
    "stripped",
    "fixed",
    "error",
    "decoded",
)
DIRECTIVES = frozenset({"skip", "xfail"})
CONFIG_KEYS = frozenset(
    {
        "dialect",
        "schema",
        "schemas",
        "import-style",
        "untyped",
        "preview",
        "type-modules",
        "rules",
        "format-command",
        "functions",
    }
)

_PY_ASSERTION = re.compile(rf"^(\s*)#\s+({'|'.join(PY_KINDS)}):(?: (.*))?$")
_SQL_ASSERTION = re.compile(rf"^(\s*)--\s+({'|'.join(SQL_KINDS)}):(?: (.*))?$")
_MISPLACED = re.compile(rf"^\s*#\s+({'|'.join(SQL_KINDS)}):")
_TRAILING_ERROR = re.compile(r"^(.*?\S)\s+#\s+error:(?: (.*))?$")
_FENCE = re.compile(r"^(\s*)(`{3,}|~{3,})\s*([\w+-]*)\s*(.*)$")
_HEADING = re.compile(r"^(#{1,6})\s+(.+?)\s*#*\s*$")
_LABEL = re.compile(r"^`([^`]+)`:\s*$")
_DIRECTIVE = re.compile(r"^<!--\s*mdtest:([\w-]+)(?:\s+(.*?))?\s*-->$")


Fail = Callable[[int, str], "ParseError"]


class ParseError(Exception):
    def __init__(self, path: Path, line: int, message: str) -> None:
        super().__init__(f"{path}:{line}: {message}")


@dataclass
class Assertion:
    kind: Kind
    expected: str
    line: int  # md line of the first comment line
    span: int = 1  # consecutive comment lines merged into ``expected``
    comment: str = "#"
    indent: str = ""
    target: int | None = None  # ``error``: md line of the code it applies to
    trailing: bool = False  # ``error`` written at the end of its code line


@dataclass
class Code:
    """Source with assertion comments removed; ``md_lines[i]`` maps line ``i + 1``."""

    text: str
    md_lines: list[int]


@dataclass
class Unit:
    lang: Literal["python", "sql"]
    lines: list[tuple[int, str]]  # (md line, text), assertion comments removed
    assertions: list[Assertion]

    @property
    def code(self) -> Code:
        return Code("\n".join(t for _, t in self.lines), [n for n, _ in self.lines])


@dataclass
class Section:
    path: Path
    titles: list[str]
    level: int
    line: int
    config: dict[str, Any] = field(default_factory=dict)
    files: dict[str, str] = field(default_factory=dict)
    directives: dict[str, str] = field(default_factory=dict)
    units: list[Unit] = field(default_factory=list)
    setup: list[tuple[int, str]] = field(default_factory=list)  # python awaiting a unit

    @property
    def name(self) -> str:
        return " - ".join(self.titles[1:] or self.titles)

    @property
    def schema_sql(self) -> str | None:
        """The ``schema`` files, joined in order."""
        schema = self.settings.get("schema", [])
        names = [schema] if isinstance(schema, str) else schema
        return "\n".join(self.files[n] for n in names) if names else None

    @property
    def assertions(self) -> list[Assertion]:
        return [a for unit in self.units for a in unit.assertions]

    @property
    def has_code(self) -> bool:
        return bool(self.units or self.setup)

    @property
    def settings(self) -> dict[str, Any]:
        settings: dict[str, Any] = self.config.get("tool", {}).get("sqlty", {})
        return settings

    def child(self, title: str, level: int, line: int) -> Section:
        return Section(
            path=self.path,
            titles=[*self.titles, title],
            level=level,
            line=line,
            config=self.config,
            files=dict(self.files),
            directives=dict(self.directives),
        )


def parse(path: Path, text: str | None = None) -> list[Section]:
    """Parse a Markdown file into sections that carry assertions."""
    lines = (path.read_text() if text is None else text).splitlines()
    root = Section(path=path, titles=[], level=0, line=0)
    stack = [root]
    sections: list[Section] = []
    label: str | None = None
    i = 0

    def fail(line: int, message: str) -> ParseError:
        return ParseError(path, line, message)

    while i < len(lines):
        line = lines[i]
        lineno = i + 1
        if fence := _FENCE.match(line):
            indent, marker, lang, info = fence.groups()
            end = _closing_fence(lines, i + 1, indent, marker)
            if end is None:
                raise fail(lineno, "unclosed code block")
            body = lines[i + 1 : end]
            if not indent and "ignore" not in info.split():
                _add_block(stack[-1], lang, label, body, lineno + 1, fail)
            label = None
            i = end + 1
            continue
        if heading := _HEADING.match(line):
            level = len(heading.group(1))
            while stack[-1].level >= level:
                stack.pop()
            section = stack[-1].child(heading.group(2), level, lineno)
            stack.append(section)
            sections.append(section)
            label = None
        elif directive := _DIRECTIVE.match(line.strip()):
            name, value = directive.groups()
            if name not in DIRECTIVES:
                raise fail(lineno, f"unknown directive mdtest:{name}")
            stack[-1].directives[name] = value or ""
        elif match := _LABEL.match(line):
            label = match.group(1)
        elif line.strip():
            label = None
        i += 1

    if root.has_code:
        raise fail(1, "code before the first heading")
    tests = [s for s in sections if s.assertions]
    seen: dict[str, int] = {}
    for s in tests:
        if s.name in seen:
            raise fail(s.line, f"duplicate test name {s.name!r} (line {seen[s.name]})")
        seen[s.name] = s.line
        _validate(s, fail)
    return tests


def _closing_fence(lines: list[str], start: int, indent: str, marker: str) -> int | None:
    for j in range(start, len(lines)):
        stripped = lines[j].strip()
        if lines[j].startswith(indent) and stripped.startswith(marker[0] * len(marker)):
            if not stripped.strip(marker[0]):
                return j
    return None


def _add_block(
    section: Section, lang: str, label: str | None, body: list[str], start: int, fail: Fail
) -> None:
    lang = {"py": "python"}.get(lang, lang)
    content = "\n".join(body)
    if label is not None:
        section.files[label] = content
    elif lang == "toml":
        section.config = _merge(section.config, _parse_config(content, start, fail))
    elif lang == "python":
        _add_python(section, body, start, fail)
    elif lang == "sql":
        _add_sql(section, body, start)


def _add_python(section: Section, body: list[str], start: int, fail: Fail) -> None:
    lines: list[tuple[int, str]] = []
    assertions: list[Assertion] = []
    pending: list[Assertion] = []
    for offset, text in enumerate(body):
        md = start + offset
        if match := _PY_ASSERTION.match(text):
            indent, kind, value = match.groups()
            assertion = Assertion(cast(Kind, kind), (value or "").rstrip(), md, indent=indent)
            if kind == "error":
                pending.append(assertion)
            _append(assertions, assertion)
            continue
        if _MISPLACED.match(text):
            raise fail(md, f"write `-- {text.split()[1]}` inside the sql block")
        if match := _TRAILING_ERROR.match(text):
            code, value = match.groups()
            assertions.append(
                Assertion("error", (value or "").rstrip(), md, target=md, trailing=True)
            )
            text = code
        if text.strip():
            for a in pending:
                a.target = md
            pending.clear()
        lines.append((md, text))
    for a in pending:
        if a.expected:
            raise fail(a.line, "`# error:` has no code line below it")
    if not assertions:
        section.setup.extend(lines)
        return
    unit = Unit("python", [*section.setup, *lines], assertions)
    if not any(text.strip() for _, text in unit.lines):
        raise fail(start, "python assertions need code")
    section.units.append(unit)
    section.setup = []


def _add_sql(section: Section, body: list[str], start: int) -> None:
    lines: list[tuple[int, str]] = []
    assertions: list[Assertion] = []
    for offset, text in enumerate(body):
        if match := _SQL_ASSERTION.match(text):
            indent, kind, value = match.groups()
            _append(
                assertions,
                Assertion(
                    cast(Kind, kind),
                    (value or "").rstrip(),
                    start + offset,
                    comment="--",
                    indent=indent,
                ),
            )
        else:
            lines.append((start + offset, text))
    if assertions:
        section.units.append(Unit("sql", lines, assertions))


def _append(assertions: list[Assertion], new: Assertion) -> None:
    """Merge a comment continuing the previous assertion of the same kind."""
    prev = assertions[-1] if assertions else None
    if (
        new.kind != "error"
        and prev is not None
        and prev.kind == new.kind
        and prev.line + prev.span == new.line
    ):
        prev.expected += "\n" + new.expected
        prev.span += 1
    else:
        assertions.append(new)


def _parse_config(content: str, line: int, fail: Fail) -> dict[str, Any]:
    from sqlty._lint import RULES, Severity

    try:
        config = tomllib.loads(content)
    except tomllib.TOMLDecodeError as e:
        raise fail(line, f"invalid TOML: {e}") from None
    if set(config) != {"tool"} or set(config["tool"]) != {"sqlty"}:
        raise fail(line, "config must be a [tool.sqlty] table")
    settings = config["tool"]["sqlty"]
    if unknown := set(settings) - CONFIG_KEYS:
        raise fail(line, f"unknown [tool.sqlty] keys: {', '.join(sorted(unknown))}")
    severities = {s.value for s in Severity}
    for rule, severity in settings.get("rules", {}).items():
        if rule not in RULES or severity not in severities:
            raise fail(line, f"invalid rule setting {rule} = {severity!r}")
    return config


def _validate(section: Section, fail: Fail) -> None:
    schema = section.settings.get("schema", [])
    for name in [schema] if isinstance(schema, str) else schema:
        if name not in section.files:
            raise fail(section.line, f"schema {name!r} has no `{name}`: block")


def _merge(base: dict[str, Any], override: dict[str, Any]) -> dict[str, Any]:
    result = dict(base)
    for key, value in override.items():
        if isinstance(result.get(key), dict) and isinstance(value, dict):
            result[key] = _merge(result[key], value)
        else:
            result[key] = value
    return result
