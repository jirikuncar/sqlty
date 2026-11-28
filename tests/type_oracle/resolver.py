"""Types a module imports from the other checked modules, read from their source."""

from __future__ import annotations

import ast
import threading
from collections.abc import Iterable
from typing import Any

from sqlty.core import ParsedSource
from tests.type_oracle.ast_fields import _ast_type
from tests.type_oracle.extract import extract_type_definitions
from tests.type_oracle.info import TypeInfo
from tests.type_oracle.symbols import ModuleTypes


class Resolver:
    """The checked modules by name, for the types one imports from another."""

    def __init__(self, sources: dict[str, ParsedSource]) -> None:
        self._sources = sources
        self._exports: dict[str, dict[str, tuple[str, str]]] = {}
        self._row_types: dict[str, dict[str, TypeInfo]] = {}
        self._types: dict[tuple[str, str], tuple[Any, int]] = {}
        self._names_of: dict[str, ModuleTypes] = {}
        # Held while a cycle sentinel is cached, so only the thread that set it can read it.
        self._lock = threading.RLock()

    def _reexports(self, module: str, parsed: ParsedSource) -> dict[str, tuple[str, str]]:
        if (exports := self._exports.get(module)) is None:
            exports = self._exports[module] = _imports(parsed, parsed.module or module)[0]
        return exports

    def row_type(
        self, parsed: ParsedSource, dotted: str
    ) -> tuple[TypeInfo, dict[str, TypeInfo]] | None:
        """The row type ``NAME`` or ``module.NAME`` that ``parsed`` imports from a checked module.

        Returns it with the row types of its module, which its nested types refer to.
        Re-exports are followed; the module is read from its source, never imported.
        """
        if (target := _target(parsed, dotted)) is None:
            return None
        return self._row_type(*target)

    def type_of(self, parsed: ParsedSource, dotted: str) -> Any:
        """The type ``NAME`` or ``module.NAME`` that ``parsed`` imports from a checked module.

        An alias or ``NewType`` is resolved from the source of its module, never imported;
        re-exports are followed. ``None`` when it is unknown.
        """
        target = _target(parsed, dotted)
        return None if target is None else self._type(*target)

    def _type(self, module: str, name: str) -> Any:
        found = self._typed(module, name, 8)
        return None if found is None else found[0]

    def _typed(self, module: str, name: str, hops: int) -> tuple[Any, int] | None:
        """The type ``name`` of ``module`` and the re-exports to it, or ``None`` past ``hops``.

        A result past ``hops`` is not cached, and a cached one is ``None`` past them, so it
        does not depend on which module asked first.
        """
        if hops < 0:
            return None
        if (parsed := self._sources.get(module)) is None:
            return None, 0
        with self._lock:
            if (known := self._types.get((module, name))) is not None:
                return known if known[1] <= hops else None
            # An import cycle sees the name as unknown while it is resolved.
            self._types[module, name] = None, 0
            if (names := self._names_of.get(module)) is None:
                names = self._names_of[module] = ModuleTypes(parsed, self, module)
            found: tuple[Any, int] | None = None, 0
            if name in names.local:
                found = (
                    _ast_type(ast.Name(name), parsed.typing_aliases, names, parsed.imported_names),
                    0,
                )
            elif (target := self._reexports(module, parsed).get(name)) is not None:
                found = self._typed(*target, hops - 1)
                found = None if found is None else (found[0], found[1] + 1)
            if found is None:
                del self._types[module, name]
            else:
                self._types[module, name] = found
            return found

    def _row_type(
        self, module: str, name: str, _depth: int = 0
    ) -> tuple[TypeInfo, dict[str, TypeInfo]] | None:
        if _depth > 8 or (parsed := self._sources.get(module)) is None:
            return None
        with self._lock:
            if (defs := self._row_types.get(module)) is None:
                # Modules that import each other's row types see none while either is read.
                self._row_types[module] = {}
                defs = extract_type_definitions(parsed, self, module)
                self._row_types[module] = defs
        if name in defs:
            return defs[name], defs
        exports = self._reexports(module, parsed)
        return self._row_type(*exports[name], _depth + 1) if name in exports else None


def _target(parsed: ParsedSource, dotted: str) -> tuple[str, str] | None:
    r"""The module and name that ``NAME`` or ``module.NAME`` of ``parsed`` refers to.

    Example:
        >>> parsed = ParsedSource("import a.b as m\nfrom c import d")
        >>> _target(parsed, "m.X"), _target(parsed, "d")
        (('a.b', 'X'), ('c', 'd'))
    """
    functions, modules = _imports(parsed, parsed.module)
    prefix, _, name = dotted.rpartition(".")
    if not prefix:
        return functions.get(name)
    head, _, rest = prefix.partition(".")
    mod = modules.get(head)
    return ((f"{mod}.{rest}" if rest else mod) if mod is not None else prefix), name


def _imports(
    parsed: ParsedSource, module: str | None
) -> tuple[dict[str, tuple[str, str]], dict[str, str]]:
    """Local names bound to imported functions ``(module, name)`` and to modules.

    Example:
        >>> _imports(ParsedSource("from a.b import f as g\\nimport a.c as m"), None)
        ({'g': ('a.b', 'f')}, {'g': 'a.b.f', 'm': 'a.c'})
    """
    return _imports_of([] if parsed.facts is None else parsed.facts.imports, module)


def _imports_of(
    entries: Iterable[tuple[str, str, str | None, int]], module: str | None
) -> tuple[dict[str, tuple[str, str]], dict[str, str]]:
    """``_imports`` of ``(local, original, from-module or None, level)``, in ``ast.walk`` order.

    A relative import resolves only within a named module.

    Example:
        >>> _imports_of([("q", "q", "db", 1)], "pkg.app")
        ({'q': ('pkg.db', 'q')}, {'q': 'pkg.db.q'})
        >>> _imports_of([("q", "q", "db", 1)], None)
        ({}, {})
        >>> _imports_of([("q", "q", "db", 3)], "pkg.app")
        ({}, {})
    """
    functions: dict[str, tuple[str, str]] = {}
    modules: dict[str, str] = {}
    package = module.rpartition(".")[0] if module else ""
    for local, original, base, level in entries:
        if base is None:
            modules[local] = original
            continue
        if level:
            if module is None:
                continue
            parts = package.split(".") if package else []
            if level - 1 > len(parts):
                continue
            parts = parts[: len(parts) - (level - 1)]
            base = ".".join([*parts, *([base] if base else [])])
        functions[local] = (base, original)
        modules[local] = f"{base}.{original}" if base else original
    return functions, modules
