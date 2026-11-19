"""The Python checks of ``check_source``: row types (TypedDict, NamedTuple, dataclass,
Pydantic, tuples) and helper call parameters against the query columns.

``check_source`` in ``sqlty.core`` runs the engine's checks, then these passes over one
``_Check`` context, then the inline suppressions.
"""

from __future__ import annotations

import sys
import typing
from collections.abc import Sequence
from typing import TYPE_CHECKING

from sqlty._core import Schema
from sqlty._core import check_modules as _check_modules
from sqlty._core import module_diagnostics as _module_diagnostics
from sqlty._helpers import HelperIndex
from sqlty._lint import Diagnostic
from sqlty._source import ParsedSource

if TYPE_CHECKING:
    pass


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
    suppress: bool = False,
) -> list[Diagnostic]:
    """Every diagnostic of ``sqlty_python::check``.

    ``variants`` turns on query variants (``find_helper_calls``): each variant of a call
    is checked as a call of its own, then ``_merge_variants`` reports at the call.
    ``coerce`` names the row types read as a Pydantic validator reads them. ``suppress``
    drops what ``# sqlty: ignore`` comments suppress and adds their ``unused-ignore``.
    """
    own = RowUsage()
    found = _rust_diagnostics(
        parsed,
        own,
        dialect,
        schema_sql,
        rules,
        import_style,
        untyped,
        helpers,
        module,
        variants,
        coerce,
        suppress,
    )
    if row_usage is not None:
        row_usage.needed |= own.needed
        row_usage.redundant += own.redundant
    return found


def _rust_diagnostics(
    parsed: ParsedSource,
    usage: RowUsage,
    dialect: str,
    schema_sql: str | Schema | None,
    rules: dict[str, str] | None,
    import_style: str | None,
    untyped: bool,
    helpers: HelperIndex | None,
    module: str | None,
    variants: int | None,
    coerce: frozenset[str],
    suppress: bool,
) -> list[Diagnostic]:
    """The diagnostics as ``sqlty_python::check`` reports them."""
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
    usage.needed.update(needed)
    usage.redundant += [
        (key, diag) for key, diag in zip(redundant, found, strict=True) if key is not None
    ]
    return found


type ModuleConfig = tuple[
    str,
    Schema | None,
    dict[str, str] | None,
    str | None,
    bool,
    frozenset[str],
    HelperIndex | None,
    int | None,
]
"""``(dialect, schema_sql, rules, import_style, untyped, coerce, helpers, variants)``."""


def check_modules(
    modules: Sequence[tuple[ParsedSource, ModuleConfig]], row_usage: RowUsage, jobs: int | None
) -> list[list[Diagnostic]]:
    """``check_source`` of every module, with ``row_usage``, in one Rust call that checks
    them on ``jobs`` threads (all cores when None)."""
    configs = _Configs()
    found = _check_modules(
        [(parsed.text, parsed.module, configs.index(config)) for parsed, config in modules],
        configs.rust(),
        version=sys.version_info[:2],
        jobs=jobs,
    )
    for diagnostics, redundant, needed in found:
        row_usage.needed.update(needed)
        row_usage.redundant += [
            (key, diag) for key, diag in zip(redundant, diagnostics, strict=True) if key is not None
        ]
    return [diagnostics for diagnostics, _, _ in found]


class _Configs:
    """Configurations, each once, by identity: a rules dict does not hash, and files share
    their settings' objects."""

    def __init__(self) -> None:
        self._known: dict[tuple[int, ...], int] = {}
        self._configs: list[ModuleConfig] = []

    def index(self, config: ModuleConfig) -> int:
        found = self._known.setdefault(tuple(map(id, config)), len(self._configs))
        if found == len(self._configs):
            self._configs.append(config)
        return found

    def rust(self) -> list[tuple[typing.Any, ...]]:
        return [
            (dialect, schema, rules, style, untyped, sorted(coerce), index and index.rust(), n)
            for dialect, schema, rules, style, untyped, coerce, index, n in self._configs
        ]
