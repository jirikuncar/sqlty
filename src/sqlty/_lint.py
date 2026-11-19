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
of its page ``sqlty/rules/<name>.md``; the engine reads them from there (``_core.rules``).
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


class RuleConfig:
    """Resolved rule configuration — maps rule names to effective severities.

    Merges defaults with user overrides from [tool.sqlty.rules].
    """

    def __init__(self, overrides: dict[str, str] | None = None) -> None:
        self._severities: dict[str, Severity] = {}

        # Start with defaults
        for name, rule in RULES.items():
            self._severities[name] = rule.default_severity

        # Apply user overrides
        if overrides:
            for name, severity_str in overrides.items():
                if name not in RULES:
                    continue  # ignore unknown rules
                try:
                    self._severities[name] = Severity(severity_str)
                except ValueError:
                    continue  # ignore invalid severity values

    def severity(self, rule_name: str) -> Severity:
        """Get the effective severity for a rule."""
        return self._severities.get(rule_name, Severity.WARN)

    def is_enabled(self, rule_name: str) -> bool:
        """Check if a rule is enabled (not ignored)."""
        return self.severity(rule_name) != Severity.IGNORE


DOCS_URL = "https://sqlty.kuncar.dev/"
"""The documentation site, which ``rule_doc`` links to."""


def rule_doc(name: str) -> str:
    """Markdown documentation of a rule, shipped as ``sqlty/rules/<name>.md``, its links
    to other pages pointing at the documentation site: the repository's relative paths
    mean nothing in a user's terminal.

    Example:
        >>> rule_doc("stale-type").splitlines()[0]
        '# stale-type'
        >>> "](https://sqlty.kuncar.dev/guide/schema-inference/)" in rule_doc("insert-missing-column")
        True
    """
    import re
    from importlib.resources import files

    text = (files("sqlty") / "rules" / f"{name}.md").read_text(encoding="utf-8")
    text = text.split("\n+++\n\n", 1)[1]

    def absolute(match: re.Match[str]) -> str:
        if (doc := match["doc"]) is not None:
            return f"]({DOCS_URL}{doc}/{match['anchor'] or ''})"
        return f"]({DOCS_URL}reference/rules/#{match['rule']})"

    link = r"\]\((?:\.\./\.\./\.\./docs/(?P<doc>[^)\s:]+)|(?P<rule>[^)\s:/]+))\.md(?P<anchor>#[^)\s]*)?\)"
    return re.sub(link, absolute, text)
