"""Helpers whose query parameter is marked ``Annotated[LiteralString, SQL]``.

A call passing a string literal to such a parameter is a checked query, with
no ``--sql`` marker or ``SQL[T]`` wrapper. The row type is the argument bound
to the parameter annotated ``type[T]`` / ``TypeForm[T]`` where ``T`` also
appears in the return annotation.

Usage:
    index = HelperIndex()
    index.add("pkg.queries", source_of_pkg_queries)
    check_source(ParsedSource(source, "pkg.app"), Settings(helpers=index))
"""

from __future__ import annotations

import sys
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from sqlty._core import (
    ModuleIndex,
    index_extended,
    index_helpers,
    module_helpers,
    module_index,
)
from sqlty._source import ParsedSource

if TYPE_CHECKING:
    from collections.abc import Sequence


@dataclass(frozen=True)
class Helper:
    """A function with a marked query parameter.

    ``positional`` lists the parameters a call may bind by position, ``keywords``
    those it may bind by name. ``row`` is ``None`` when no ``type[T]`` parameter
    shares ``T`` with the return type. A method is named ``Class.method`` and its
    ``self`` / ``cls`` is not listed; ``kind`` is ``instance`` for a method that is
    only callable on an instance (a call through the class would shift the arguments).
    ``configured`` marks a helper of ``[[tool.sqlty.functions]]``, and ``database`` the
    named database its queries are read against.
    """

    name: str
    positional: tuple[str, ...]
    keywords: frozenset[str]
    query: str
    row: str | None
    kind: str = "function"
    params: str | None = None
    rest: str | None = None
    named: str | None = None
    configured: bool = False
    database: str | None = None


def _helper_of(name: str, fields: tuple[Any, ...]) -> Helper:
    """The ``Helper`` of what Rust reads of one (``ModuleIndex.helpers``)."""
    positional, keywords, *rest = fields
    return Helper(name, tuple(positional), frozenset(keywords), *rest)


class HelperIndex:
    """Helpers of the modules of a checked tree, by module name.

    Example:
        >>> index = HelperIndex()
        >>> index.add("pkg.db", "from typing import Annotated\\nfrom sqlty import SQL\\ndef run(q: Annotated[str, SQL]): ...")
        True
        >>> sorted(index.modules["pkg.db"])
        ['run']
    """

    def __init__(self) -> None:
        self._lock = threading.RLock()
        # How the index was filled, to build it again in Rust.
        self._steps: list[tuple[str, str, str, str | None, list[Path], str | None]] = []
        self._rust: tuple[int, ModuleIndex] | None = None
        self._forked: tuple[HelperIndex, int] | None = None

    @property
    def modules(self) -> dict[str, dict[str, Helper]]:
        """The helpers by module and name (``Class.method`` for a method)."""
        return {
            module: {name: _helper_of(name, fields) for name, fields in found.items()}
            for module, found in index_helpers(self.rust()).items()
        }

    def rust(self) -> ModuleIndex:
        """The index as Rust reads it."""
        with self._lock:
            if self._rust is None or self._rust[0] != len(self._steps):
                parent, forked_at = self._forked or (None, 0)
                # A fork of an index that has not changed since shares what Rust read of it.
                if parent is not None and len(parent._steps) == forked_at:
                    built = index_extended(parent.rust(), self._steps[forked_at:])
                else:
                    built = module_index(sys.version_info[:2], self._steps)
                self._rust = len(self._steps), built
            return self._rust[1]

    def fork(self) -> HelperIndex:
        """A copy whose later ``register`` calls do not reach this index."""
        other = HelperIndex()
        other._lock = self._lock
        other._steps = list(self._steps)
        other._forked = self, len(self._steps)
        return other

    def add(self, module: str, source: str | ParsedSource) -> bool:
        """Index the helpers of ``source`` as module ``module``; whether it has any."""
        parsed = ParsedSource.of(source)
        if parsed.module is None:
            parsed.module = module
        self._steps.append(("add", module, parsed.text, parsed.module, [], None))
        return parsed.may_define_helpers and bool(module_helpers(parsed.text, sys.version_info[:2]))

    def register(
        self,
        import_path: str,
        query: str,
        row: str | None = None,
        *,
        roots: Sequence[Path] = (),
        database: str | None = None,
    ) -> None:
        """Register ``import_path`` (``pkg.mod.func`` or ``pkg.mod.Class.method``) as a helper.

        Used for ``[[tool.sqlty.functions]]``: the function needs no marked parameter. Its
        positional parameters are read from its definition in ``pkg.mod``, indexed or else
        read (never imported) from ``pkg/mod.py`` or ``pkg/mod/__init__.py`` under the first
        of ``roots`` holding it, following names ``pkg.mod`` re-exports (``from .core import
        Connection``); otherwise ``query`` and ``row`` can only be passed by keyword. Of
        ``@overload`` definitions, the first naming both is used unless the implementation
        names them.

        Example:
            >>> index = HelperIndex()
            >>> index.register("db.run", "sql", "form")
            >>> index.modules["db"]["run"].keywords == {"sql", "form"}
            True
        """
        self._steps.append(("register", import_path, query, row, list(roots), database))
