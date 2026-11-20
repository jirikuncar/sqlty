"""The Rust checks behind ``check_source``, and the ``redundant-optional`` reports that
``RowUsage`` settles across the modules a caller checks."""

from __future__ import annotations

import sys

from sqlty._core import Schema
from sqlty._core import module_diagnostics as _module_diagnostics
from sqlty._helpers import HelperIndex
from sqlty._lint import Diagnostic
from sqlty._source import ParsedSource


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
    row_usage: RowUsage,
    variants: int | None = None,
    coerce: frozenset[str] = frozenset(),
    suppress: bool = False,
) -> list[Diagnostic]:
    """Every diagnostic of ``sqlty_python::check``, recording in ``row_usage`` what it needs.

    ``variants`` turns on query variants: each variant of a call is checked as a call of
    its own and reported at the call.
    ``coerce`` names the row types read as a Pydantic validator reads them. ``suppress``
    drops what ``# sqlty: ignore`` comments suppress and adds their ``unused-ignore``.
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
        suppress=suppress,
    )
    row_usage.needed.update(needed)
    row_usage.redundant += [
        (key, diag) for key, diag in zip(redundant, found, strict=True) if key is not None
    ]
    return found
