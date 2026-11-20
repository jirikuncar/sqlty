"""Lint rule registry — ty-style per-rule severity configuration.

Rules use kebab-case names and have three severity levels:
- "error": hard failure (exit code 1)
- "warn": warning (exit code 0, shown in output)
- "ignore": suppressed entirely

Configuration via sqlty.toml (``[rules]``) or pyproject.toml:

    [tool.sqlty.rules]
    column-name-mismatch = "error"
    redundant-optional = "warn"

A rule's name, default severity, group and summary are written once, in the front matter
of its page ``docs/reference/rules/<name>.md``; the engine reads them from there (``_core.rules``).
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import TypedDict

from sqlty._core import name_rules, rules


class Severity(Enum):
    """Lint rule severity level."""

    ERROR = "error"
    WARN = "warn"
    IGNORE = "ignore"


@dataclass(frozen=True)
class LintRule:
    """A single lint rule definition."""

    name: str
    default_severity: Severity
    fixable: bool
    description: str
    group: str


# Rules that explain why a column is untyped, as the engine lists them; the entries in
# ``RULES`` below must cover them (``tests/test_name_rules.py``).
NAME_RULES: tuple[str, ...] = tuple(name_rules())

# --- Rule definitions ---

RULES: dict[str, LintRule] = {
    name: LintRule(
        name=name,
        default_severity=Severity(severity),
        fixable=fixable,
        description=summary,
        group=group,
    )
    for name, severity, fixable, summary, group in rules()
}


class Diagnostic(TypedDict):
    """One ``check_source`` result: a plain dict, so ``diag["rule"]`` keeps working.

    ``severity`` is ``"error"`` or ``"warn"``. ``line`` is 1-based: where the call or the
    string it is about starts, the line a ``# sqlty: ignore`` comment silences it on.
    ``start`` and ``end`` are the byte offsets (UTF-8) of what it is about: a row type, a
    query, a parameter type or a comment. ``fix`` holds the ``(start, end, replacement)``
    byte edits of ``sqlty check --fix``; it is empty when no fix applies.
    """

    rule: str
    message: str
    severity: str
    line: int
    start: int
    end: int
    fix: list[tuple[int, int, str]]


DOCS_URL = "https://sqlty.kuncar.dev/"
