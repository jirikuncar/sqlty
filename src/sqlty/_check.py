"""The check of one module: ``sqlty_python::check`` and the inline suppressions.

``check_source`` in ``sqlty.core`` asks ``diagnostics`` for what Rust reports of the
module (the engine's checks, row types, helper and driver calls), then drops what a
``# sqlty: ignore`` comment suppresses.
"""

from __future__ import annotations

import sys
from typing import TYPE_CHECKING

from sqlty._core import module_diagnostics as _module_diagnostics

if TYPE_CHECKING:
    from sqlty._core import Schema
    from sqlty._helpers import HelperIndex
    from sqlty._lint import Diagnostic
    from sqlty._source import Ignore, ParsedSource


class RowUsage:
    """Which fields of the row types a run checks some query needs ``| None`` for.

    ``redundant-optional`` is a question about a row type, not a query: a field is
    redundant only when no query that loads the type can return NULL for it.
    """

    def __init__(self) -> None:
        self.needed: set[tuple[str | None, str, str]] = set()
        self.redundant: list[tuple[tuple[str | None, str, str], Diagnostic]] = []

    def filter(self, diagnostics: list[Diagnostic]) -> list[Diagnostic]:
        """``diagnostics`` without the ``redundant-optional`` reports another query refutes."""
        refuted = {id(diag) for key, diag in self.redundant if key in self.needed}
        return [diag for diag in diagnostics if id(diag) not in refuted]


def diagnostics(
    parsed: ParsedSource,
    *,
    dialect: str,
    schema_sql: str | Schema | None,
    rules: dict[str, str] | None,
    import_style: str | None,
    untyped: bool,
    helpers: HelperIndex | None,
    module: str | None,
    row_usage: RowUsage | None = None,
    variants: int | None = None,
    coerce: frozenset[str] = frozenset(),
) -> list[Diagnostic]:
    """Every diagnostic before inline suppression, by line.

    ``variants`` turns on query variants: each variant of a query built in branches is
    checked, and reported at the call. ``coerce`` names the row types read as a Pydantic
    validator reads them. ``row_usage`` collects the ``redundant-optional`` questions.
    """
    found, redundant, needed = _module_diagnostics(
        parsed.text,
        version=sys.version_info[:2],
        dialect=dialect,
        schema_sql=schema_sql,
        rules=rules,
        import_style=import_style,
        untyped=untyped,
        module=module,
        coerce=sorted(coerce),
        index=None if helpers is None else helpers.rust(),
        name=parsed.module,
        variants=variants,
    )
    if row_usage is not None:
        row_usage.needed.update(needed)
        row_usage.redundant += [
            (key, diag) for key, diag in zip(redundant, found, strict=True) if key is not None
        ]
    return found


def apply_suppressions(
    ignores: dict[int, Ignore],
    diagnostics: list[Diagnostic],
    rules: dict[str, str] | None,
    marker: str = "#",
) -> list[Diagnostic]:
    """Drop diagnostics suppressed by a comment on their line; add ``unused-ignore``.

    ``marker`` starts a comment in the checked language: ``#``, or ``--`` in SQL.
    """
    from sqlty._lint import RuleConfig

    if not ignores:
        return diagnostics

    kept: list[Diagnostic] = []
    used: set[tuple[int, str | None]] = set()
    for diag in diagnostics:
        ignore = ignores.get(diag["line"])
        if ignore is None or (ignore.rules is not None and diag["rule"] not in ignore.rules):
            kept.append(diag)
        else:
            used.add((ignore.line, None if ignore.rules is None else diag["rule"]))

    rule_config = RuleConfig(overrides=rules)
    if rule_config.is_enabled("unused-ignore"):
        severity = rule_config.severity("unused-ignore").value
        for ignore in ignores.values():
            unused: list[str | None]
            if ignore.rules is None:
                unused = [None] if (ignore.line, None) not in used else []
            else:
                unused = [name for name in ignore.rules if (ignore.line, name) not in used]
            for name in unused:
                written = f"{marker} sqlty: ignore" + ("" if name is None else f"[{name}]")
                kept.append(
                    {
                        "rule": "unused-ignore",
                        "message": f"'{written}' suppresses no diagnostic",
                        "severity": severity,
                        "line": ignore.line,
                        "start": ignore.start,
                        "end": ignore.end,
                        "fix": [],
                    }
                )
    return kept
