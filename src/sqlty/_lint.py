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
    nullable-mismatch = "error"         # default
    json-shape-mismatch = "error"       # default
    redundant-optional = "ignore"       # default
    literal-mismatch = "error"          # default
    redundant-literal = "ignore"        # default
    typeddict-key-mismatch = "error"    # default
    typeddict-inheritance = "error"     # default
    select-star = "warn"                # default
    select-star-no-schema = "error"     # default
    stale-rewrite = "warn"              # default
    stale-type = "warn"                 # default
    inline-sql-comment = "warn"         # default
    fstring-skipped = "warn"            # default
    tstring-skipped = "warn"            # default
    unchecked-query = "warn"            # default
    untyped-query = "warn"              # default
    unparsed-query = "ignore"           # default
    unknown-table = "ignore"            # default
    unknown-column = "ignore"           # default
    ambiguous-column = "ignore"         # default
    unknown-function = "ignore"         # default
    unknown-type = "ignore"             # default
    unsupported-syntax = "ignore"       # default
    unused-ignore = "ignore"            # default
    param-count-mismatch = "error"      # default
    param-type-mismatch = "error"       # default
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import TypedDict

from sqlty._core import name_rules


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


# Rules that explain why a column is untyped, as the engine lists them; the entries in
# ``RULES`` below must cover them (``tests/test_name_rules.py``).
NAME_RULES: tuple[str, ...] = tuple(name_rules())

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
        description="Annotated NamedTuple field name doesn't match query column alias",
    ),
    "typeddict-key-mismatch": LintRule(
        name="typeddict-key-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="Query columns don't fit the fields of a TypedDict, dataclass or Pydantic model",
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
    "nullable-mismatch": LintRule(
        name="nullable-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="Row type field is not optional but its query column can be NULL",
    ),
    "json-shape-mismatch": LintRule(
        name="json-shape-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="Row type field cannot hold the JSON a query column builds",
    ),
    "redundant-optional": LintRule(
        name="redundant-optional",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="Row type field is optional but its query column cannot be NULL",
    ),
    "literal-mismatch": LintRule(
        name="literal-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="Row type field is a Literal that does not cover the values its column can take",
    ),
    "redundant-literal": LintRule(
        name="redundant-literal",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="Row type field is a Literal that lists values its column can never take",
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
    "mysql-sql-marker": LintRule(
        name="mysql-sql-marker",
        default_severity=Severity.WARN,
        fixable=True,
        description='MySQL rejects the "--sql" marker; use "-- sql"',
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
        description="t-string with --sql prefix skipped (a field with a format spec or conversion changes the SQL)",
    ),
    "unchecked-query": LintRule(
        name="unchecked-query",
        default_severity=Severity.WARN,
        fixable=False,
        description="--sql query is not plain string literals (raw, bytes, an expression), so it is not checked",
    ),
    "untyped-query": LintRule(
        name="untyped-query",
        default_severity=Severity.WARN,
        fixable=True,
        description="SQL[UNTYPED] query has no inferable row type; add casts or a schema",
    ),
    "unparsed-query": LintRule(
        name="unparsed-query",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="--sql query, SQL[T] query or Annotated helper query that does not parse",
    ),
    "unknown-table": LintRule(
        name="unknown-table",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="Query reads a table that the schema does not define",
    ),
    "unknown-column": LintRule(
        name="unknown-column",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="Query reads a column that its table does not have",
    ),
    "ambiguous-column": LintRule(
        name="ambiguous-column",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="Unqualified column exists in more than one table of the query",
    ),
    "unknown-function": LintRule(
        name="unknown-function",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="Function result is untyped because sqlty does not know the function",
    ),
    "unknown-type": LintRule(
        name="unknown-type",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="Cast or column type has no Python type",
    ),
    "unsupported-syntax": LintRule(
        name="unsupported-syntax",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="Query parses, but a construct in it is not typed, so the row type is not trusted",
    ),
    "param-count-mismatch": LintRule(
        name="param-count-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="A call passes a different number of parameters, or different names, than its query has placeholders",
    ),
    "param-type-mismatch": LintRule(
        name="param-type-mismatch",
        default_severity=Severity.ERROR,
        fixable=False,
        description="A call passes an argument whose type can't be the type its query parameter takes",
    ),
    "unused-ignore": LintRule(
        name="unused-ignore",
        default_severity=Severity.IGNORE,
        fixable=False,
        description="# sqlty: ignore comment, or rule in one, that suppresses no diagnostic",
    ),
}


class Diagnostic(TypedDict):
    """One ``check_source`` result: a plain dict, so ``diag["rule"]`` keeps working.

    ``severity`` is ``"error"`` or ``"warn"``; ``line`` is 1-based.
    """

    rule: str
    message: str
    severity: str
    line: int


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


def rule_doc(name: str) -> str:
    """Markdown documentation of a rule, shipped as ``sqlty/rules/<name>.md``.

    Example:
        >>> rule_doc("stale-type").splitlines()[0]
        '# stale-type'
    """
    from importlib.resources import files

    return (files("sqlty") / "rules" / f"{name}.md").read_text(encoding="utf-8")
