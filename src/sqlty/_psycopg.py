"""Render ``psycopg.sql`` compositions built only from literals.

The expression is rebuilt from the AST with the real ``psycopg.sql`` classes
(no user code runs) and rendered with ``Composable.as_string(None)``, which
needs no connection since psycopg 3.2. ``sql.Literal(x)`` renders as ``NULL``.

Usage:
    scan = ParsedSource(text).psycopg  # built from the one walk of the module
    scan.render(expr)  # str, UNCHECKED, or None when not a psycopg query
"""

from __future__ import annotations

import ast
from typing import Any, Final

_CLASSES = frozenset({"SQL", "Identifier", "Literal", "Placeholder", "Composed"})
_METHODS = frozenset({"format", "join"})
_Pos = tuple[int, int]

UNCHECKED: Final = "unchecked"


class _Dynamic(Exception):
    """A part that is not a literal."""


class _Invalid(Exception):
    """A composition psycopg itself rejects."""


def _dotted(node: ast.expr) -> str | None:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute) and (base := _dotted(node.value)):
        return f"{base}.{node.attr}"
    return None


_Scope = ast.FunctionDef | ast.AsyncFunctionDef | ast.Lambda | ast.ClassDef


def _span(node: _Scope) -> tuple[_Pos, _Pos]:
    return (
        (node.lineno, node.col_offset),
        (node.end_lineno or 0, node.end_col_offset or 0),
    )


def _params(node: ast.FunctionDef | ast.AsyncFunctionDef | ast.Lambda) -> set[str]:
    args = node.args
    names = {a.arg for a in [*args.posonlyargs, *args.args, *args.kwonlyargs]}
    names.update(a.arg for a in (args.vararg, args.kwarg) if a is not None)
    return names


class PsycopgScan:
    """What a module walk collects to follow names and imports.

    Example:
        >>> from sqlty._source import ParsedSource
        >>> parsed = ParsedSource("from psycopg import sql\\nQ = sql.SQL('SELECT {}').format(sql.Identifier('t'))")
        >>> parsed.psycopg.render(parsed.calls[0])
        'SELECT "t"'
    """

    def __init__(
        self,
        imports: list[ast.Import | ast.ImportFrom],
        scopes: list[_Scope],
        stores: dict[str, list[ast.Name]],
        values: dict[int, ast.expr],
    ) -> None:
        self.imports = imports
        self.scopes = scopes
        self.stores = stores
        self.values = values
        self._table: dict[str, str] | None = None
        self._sql: Any = None

    def render(self, expr: ast.expr) -> str | None:
        """The SQL of a psycopg composition, ``UNCHECKED`` when a part is dynamic.

        ``None`` when ``expr`` is no psycopg composition or psycopg is missing.
        """
        try:
            from psycopg import sql
        except ImportError:
            return None
        self._sql = sql
        run = _Run(self)
        try:
            value = run.value(expr)
            if not isinstance(value, sql.Composable):
                return None
            return value.as_string(None)
        except _Dynamic:
            return UNCHECKED if run.seen else None
        except _Invalid, TypeError, ValueError, NotImplementedError, AttributeError:
            return None

    @property
    def table(self) -> dict[str, str]:
        """Local names bound by ``psycopg`` imports, to their dotted target."""
        if self._table is None:
            table: dict[str, str] = {}
            for node in self.imports:
                if isinstance(node, ast.Import):
                    for alias in node.names:
                        if alias.name.split(".")[0] != "psycopg":
                            continue
                        if alias.asname:
                            table[alias.asname] = alias.name
                        else:
                            table["psycopg"] = "psycopg"
                elif not node.level and (node.module or "").split(".")[0] == "psycopg":
                    for alias in node.names:
                        table[alias.asname or alias.name] = f"{node.module}.{alias.name}"
            self._table = table
        return self._table

    def binding(self, name: ast.Name) -> ast.expr | None:
        """The value of the single assignment ``name`` refers to, if there is one.

        Looks at the enclosing functions, innermost first, then the module.
        """
        pos = (name.lineno, name.col_offset)
        enclosing = [scope for scope in self.scopes if (span := _span(scope))[0] <= pos < span[1]]
        enclosing.sort(key=lambda scope: _span(scope)[0], reverse=True)
        functions = [s for s in enclosing if not isinstance(s, ast.ClassDef)]
        for scope in [*functions, None]:
            if isinstance(scope, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)):
                if name.id in _params(scope):
                    return None
            found = [s for s in self.stores.get(name.id, []) if self._owner(s) is scope]
            if not found:
                continue
            if len(found) == 1:
                return self.values.get(id(found[0]))
            return None
        return None

    def _owner(self, store: ast.Name) -> _Scope | None:
        """The innermost function or class containing ``store``, ``None`` for the module."""
        pos = (store.lineno, store.col_offset)
        best: _Scope | None = None
        for scope in self.scopes:
            span = _span(scope)
            if span[0] <= pos < span[1] and (best is None or _span(best)[0] <= span[0]):
                best = scope
        return best


class _Run:
    """One evaluation of an expression."""

    def __init__(self, scan: PsycopgScan) -> None:
        self.scan = scan
        self.sql = scan._sql
        self.seen = False
        self.depth = 0

    def value(self, node: ast.expr) -> Any:
        self.depth += 1
        try:
            if self.depth > 64:
                raise _Dynamic
            return self._value(node)
        finally:
            self.depth -= 1

    def _value(self, node: ast.expr) -> Any:
        if isinstance(node, ast.Constant):
            return node.value
        if isinstance(node, ast.Name):
            if (target := self.scan.binding(node)) is None:
                raise _Dynamic
            return self.value(target)
        if isinstance(node, (ast.List, ast.Tuple)):
            if any(isinstance(item, ast.Starred) for item in node.elts):
                raise _Dynamic
            return [self.value(item) for item in node.elts]
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
            return self.value(node.left) + self.value(node.right)
        if isinstance(node, ast.Call):
            return self._call(node)
        raise _Dynamic

    def _symbol(self, func: ast.expr) -> str | None:
        """The ``psycopg.sql`` class ``func`` names, through any import form."""
        if (dotted := _dotted(func)) is None:
            return None
        head, _, rest = dotted.partition(".")
        if (target := self.scan.table.get(head)) is None:
            return None
        full = f"{target}.{rest}" if rest else target
        module, _, name = full.rpartition(".")
        return name if module == "psycopg.sql" and name in _CLASSES else None

    def _call(self, node: ast.Call) -> Any:
        if (name := self._symbol(node.func)) is not None:
            self.seen = True
            if name == "Literal":
                return self.sql.SQL("NULL")
            args, kwargs = self._arguments(node)
            return getattr(self.sql, name)(*args, **kwargs)
        if isinstance(node.func, ast.Attribute) and node.func.attr in _METHODS:
            receiver = self.value(node.func.value)
            if not isinstance(receiver, (str, self.sql.SQL)):
                raise _Invalid
            args, kwargs = self._arguments(node)
            try:
                return getattr(receiver, node.func.attr)(*args, **kwargs)
            except (TypeError, ValueError, IndexError, KeyError) as error:
                raise _Invalid from error
        raise _Dynamic

    def _arguments(self, node: ast.Call) -> tuple[list[Any], dict[str, Any]]:
        if any(isinstance(arg, ast.Starred) for arg in node.args) or any(
            keyword.arg is None for keyword in node.keywords
        ):
            raise _Dynamic
        args = [self.value(arg) for arg in node.args]
        kwargs = {kw.arg: self.value(kw.value) for kw in node.keywords if kw.arg}
        return args, kwargs
