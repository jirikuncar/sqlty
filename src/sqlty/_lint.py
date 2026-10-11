"""Lint rule registry — ty-style per-rule severity configuration.

Rules use kebab-case names and have three severity levels:
- "error": hard failure (exit code 1)
- "warn": warning (exit code 0, shown in output)
- "ignore": suppressed entirely

Configuration via pyproject.toml:

    [tool.sqlty.rules]
    column-count-mismatch = "error"     # default
    column-type-mismatch = "error"      # default
    column-name-mismatch = "warn"       # default
    typeddict-key-mismatch = "error"    # default
    typeddict-inheritance = "error"     # default
    select-star = "warn"                # default
    select-star-no-schema = "error"     # default
    stale-rewrite = "warn"              # default
    stale-type = "warn"                 # default
    inline-sql-comment = "warn"         # default
    fstring-skipped = "warn"            # default
    tstring-skipped = "warn"            # default
    untyped-query = "warn"              # default
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from enum import Enum
from typing import Any

# ``--sql SELECT ...``: a space after the marker, then the start of a SQL statement,
# which makes the whole query a SQL comment (``inline-sql-comment``). Each keyword
# needs what its statement needs (``DELETE FROM``, ``UPDATE t SET``, ...), so prose
# such as ``"--sql delete me later"`` does not match. No parser is involved, and
# ``INLINE_SQL_COMMENT`` in ``rust/src/checker.rs`` is the same pattern.
INLINE_SQL_COMMENT = re.compile(
    r"--sql[ \t]+(?i:"
    r"SELECT\s+[^\s;]"
    r"|WITH\s+(?:RECURSIVE\s+)?\w+[^;]*?\bAS\s*\("
    r"|INSERT\s+INTO\b"
    r"|UPDATE\s+[\w.\"]+(?:\s+(?:AS\s+)?\w+)?\s+SET\b"
    r"|DELETE\s+FROM\b"
    r"|VALUES\s*\("
    r"|(?:CREATE|ALTER|DROP)\s+(?:(?:OR\s+REPLACE|TEMP|TEMPORARY|UNIQUE|MATERIALIZED)\s+)*"
    r"(?:TABLE|VIEW|INDEX|SCHEMA|SEQUENCE|FUNCTION|TYPE|EXTENSION|DATABASE|TRIGGER|PROCEDURE|ROLE)\b"
    r"|TRUNCATE\s+(?:TABLE\s+)?\w"
    r"|MERGE\s+INTO\b"
    r"|EXPLAIN\s+(?:(?:ANALYZE|VERBOSE)\s+)*(?:SELECT|WITH|INSERT|UPDATE|DELETE|VALUES)\b"
    r")"
)


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


# --- Rule definitions ---

RULES: dict[str, LintRule] = {
    "column-count-mismatch": LintRule(
        name="column-count-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="Annotated type has different number of fields than query columns",
    ),
    "column-type-mismatch": LintRule(
        name="column-type-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="Annotated type field has incompatible type with query column",
    ),
    "column-name-mismatch": LintRule(
        name="column-name-mismatch",
        default_severity=Severity.WARN,
        fixable=False,
        description="Annotated type field name doesn't match query column alias",
    ),
    "typeddict-key-mismatch": LintRule(
        name="typeddict-key-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="Query columns don't fit the keys of a closed or extra_items TypedDict",
    ),
    "typeddict-inheritance": LintRule(
        name="typeddict-inheritance",
        default_severity=Severity.ERROR,
        fixable=False,
        description=(
            "TypedDict subclass changes openness or adds keys a closed or extra_items base "
            "forbids (PEP 728)"
        ),
    ),
    "select-star": LintRule(
        name="select-star",
        default_severity=Severity.WARN,
        fixable=True,
        description="SELECT * used — expand to explicit column list (joins need a manual edit)",
    ),
    "select-star-no-schema": LintRule(
        name="select-star-no-schema",
        default_severity=Severity.ERROR,
        fixable=False,
        description="SELECT * used but no schema available to expand",
    ),
    "stale-rewrite": LintRule(
        name="stale-rewrite",
        default_severity=Severity.WARN,
        fixable=True,
        description="Source has --sql strings that aren't wrapped in SQL[T]",
    ),
    "stale-type": LintRule(
        name="stale-type",
        default_severity=Severity.WARN,
        fixable=True,
        description="SQL[tuple[...]] type doesn't match current schema (drift)",
    ),
    "inline-sql-comment": LintRule(
        name="inline-sql-comment",
        default_severity=Severity.WARN,
        fixable=True,
        description='--sql followed by space makes entire query a SQL comment; use "--sql\\nSELECT ..." or run sqlty rewrite',
    ),
    "fstring-skipped": LintRule(
        name="fstring-skipped",
        default_severity=Severity.WARN,
        fixable=False,
        description="f-string with --sql prefix skipped (cannot be statically rewritten)",
    ),
    "tstring-skipped": LintRule(
        name="tstring-skipped",
        default_severity=Severity.WARN,
        fixable=False,
        description="t-string with --sql prefix skipped (cannot be statically rewritten)",
    ),
    "untyped-query": LintRule(
        name="untyped-query",
        default_severity=Severity.WARN,
        fixable=True,
        description="SQL[UNTYPED] query has no inferable row type; add casts or a schema",
    ),
}


@dataclass
class Diagnostic:
    """A single lint diagnostic emitted by the checker."""

    rule: str
    message: str
    severity: Severity
    line: int | None = None
    col: int | None = None

    def __str__(self) -> str:
        loc = f":{self.line}" if self.line else ""
        return f"[{self.rule}]{loc} {self.message}"


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

    @classmethod
    def from_config(cls, config: dict[str, Any]) -> RuleConfig:
        """Create RuleConfig from a [tool.sqlty] config dict."""
        rules = config.get("rules", {})
        return cls(overrides=rules)


def rule_doc(name: str) -> str:
    """Markdown documentation of a rule, shipped as ``sqlty/rules/<name>.md``.

    Example:
        >>> rule_doc("stale-type").splitlines()[0]
        '# stale-type'
    """
    from importlib.resources import files

    return (files("sqlty") / "rules" / f"{name}.md").read_text(encoding="utf-8")
