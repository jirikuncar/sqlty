"""Driver calls that pass a query and its parameters: ``cursor.execute(query, params)``.

A receiver is recognised only when the file shows what it is: a variable bound to
``psycopg.connect(...)``, ``sqlite3.connect(...)`` or ``asyncpg.connect(...)`` (also as
``with ... as name``), to ``<connection>.cursor()``, or annotated with a driver class. The
query is a string literal, ``SQL[T]("...")`` of one, or a name assigned exactly one of
those. Anything else is skipped.

Names are resolved per scope, and a name a loop, comprehension, unpacking, import or
unannotated parameter binds is unknown.

Example:
    >>> src = '''
    ... import sqlite3
    ... conn = sqlite3.connect("db")
    ... conn.execute("SELECT 1 WHERE ? = 1", (1, 2))
    ... '''
    >>> [(c.line, c.query, [a.key for a in c.params or ()]) for c in find_driver_calls(src)]
    [(4, 'SELECT 1 WHERE ? = 1', [1, 2])]
"""

from __future__ import annotations

import ast
from collections import defaultdict

from sqlty._helpers import HelperCall
from sqlty._params import ParamArg, arg_type, literal_params
from sqlty._source import ParsedSource

# Driver factories and classes, by the dotted name they are imported as, to the driver.
_CONNECTIONS = {
    "psycopg.connect": "psycopg",
    "psycopg.Connection": "psycopg",
    "psycopg.AsyncConnection": "psycopg",
    "psycopg.AsyncConnection.connect": "psycopg",
    "psycopg.Connection.connect": "psycopg",
    "sqlite3.connect": "sqlite3",
    "sqlite3.Connection": "sqlite3",
    "asyncpg.connect": "asyncpg",
    "asyncpg.Connection": "asyncpg",
    "asyncpg.create_pool": "asyncpg",
    "asyncpg.Pool": "asyncpg",
}
_CURSORS = {
    "psycopg.Cursor": "psycopg",
    "psycopg.AsyncCursor": "psycopg",
    "sqlite3.Cursor": "sqlite3",
}
_METHODS = {
    "psycopg": {"execute", "executemany"},
    "sqlite3": {"execute", "executemany"},
    "asyncpg": {"execute", "fetch", "fetchrow", "fetchval", "executemany"},
}
_PARAM_KEYWORD = {"psycopg": "params", "sqlite3": "parameters"}


def _dotted(node: ast.expr) -> str | None:
    parts: list[str] = []
    while isinstance(node, ast.Attribute):
        parts.append(node.attr)
        node = node.value
    return ".".join([node.id, *reversed(parts)]) if isinstance(node, ast.Name) else None


def _qualified(name: str | None, parsed: ParsedSource) -> str | None:
    """``name`` with its first part replaced by what it was imported as."""
    if name is None:
        return None
    head, _, rest = name.partition(".")
    imported = parsed.imported_names.get(head)
    return None if imported is None else f"{imported}.{rest}" if rest else imported


_Binding = tuple[ast.expr | None, ast.expr | None]  # (value, annotation); both None: unknown
_Scope = ast.AST  # the module, or a function


class _Scopes:
    """What names mean in each scope of a file.

    A name takes its meaning from the innermost enclosing function (or the module) that
    binds it. Names bound by anything but a plain assignment, an annotated parameter or
    ``with ... as name`` (loops, comprehensions, unpacking, imports, definitions, an
    unannotated parameter) are unknown.
    """

    def __init__(self, parsed: ParsedSource) -> None:
        assert parsed.tree is not None
        self.parsed = parsed
        self.tree: _Scope = parsed.tree
        self._bindings: dict[int, dict[str, list[_Binding]]] = {}
        self._resolving: set[tuple[int, str]] = set()

    def chain(self, call: ast.Call) -> list[_Scope]:
        """The scopes around ``call``, outermost (the module) first."""
        inner = [
            s
            for s in self.parsed.scopes_of(call)
            if isinstance(s, (ast.FunctionDef, ast.AsyncFunctionDef))
        ]
        return [self.tree, *inner]

    def _of(self, scope: _Scope) -> dict[str, list[_Binding]]:
        if (found := self._bindings.get(id(scope))) is not None:
            return found
        names: dict[str, list[_Binding]] = defaultdict(list)
        self._bindings[id(scope)] = names
        unknown: _Binding = (None, None)

        def bind(target: ast.AST, binding: _Binding = unknown) -> None:
            if isinstance(target, ast.Name):
                names[target.id].append(binding)
            else:
                for node in ast.walk(target):
                    if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Store):
                        names[node.id].append(unknown)

        if isinstance(scope, (ast.FunctionDef, ast.AsyncFunctionDef)):
            args = scope.args
            for arg in [*args.posonlyargs, *args.args, *args.kwonlyargs, args.vararg, args.kwarg]:
                if arg is not None:
                    names[arg.arg].append((None, arg.annotation))
        pending = list(ast.iter_child_nodes(scope))
        while pending:
            node = pending.pop()
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                names[node.name].append(unknown)
                continue
            if isinstance(node, ast.Lambda):
                continue
            if isinstance(node, ast.Assign):
                for target in node.targets:
                    bind(target, (node.value, None) if isinstance(target, ast.Name) else unknown)
            elif isinstance(node, ast.AnnAssign):
                bind(node.target, (node.value, node.annotation))
            elif isinstance(node, (ast.With, ast.AsyncWith)):
                for item in node.items:
                    if item.optional_vars is not None:
                        bind(item.optional_vars, (item.context_expr, None))
            elif isinstance(
                node, (ast.For, ast.AsyncFor, ast.comprehension, ast.AugAssign, ast.NamedExpr)
            ):
                bind(node.target)
            elif isinstance(node, ast.ExceptHandler) and node.name:
                names[node.name].append(unknown)
            elif isinstance(node, (ast.Import, ast.ImportFrom)):
                for alias in node.names:
                    names[(alias.asname or alias.name).partition(".")[0]].append(unknown)
            pending.extend(ast.iter_child_nodes(node))
        return names

    def _lookup(self, name: str, scopes: list[_Scope]) -> list[_Binding] | None:
        for scope in reversed(scopes):
            if (found := self._of(scope).get(name)) is not None:
                return found
        return None

    def driver(self, name: str, scopes: list[_Scope]) -> str | None:
        """The driver of the connection or cursor ``name`` is, in ``scopes``, when every binding agrees."""
        key = (id(scopes[-1]), name)
        if key in self._resolving or (bindings := self._lookup(name, scopes)) is None:
            return None
        self._resolving.add(key)
        try:
            drivers = {self._kind(value, annotation, scopes) for value, annotation in bindings}
        finally:
            self._resolving.discard(key)
        return drivers.pop() if len(drivers) == 1 else None

    def _kind(
        self, value: ast.expr | None, annotation: ast.expr | None, scopes: list[_Scope]
    ) -> str | None:
        if annotation is not None:
            name = _qualified(_dotted(annotation), self.parsed)
            if name in _CONNECTIONS:
                return _CONNECTIONS[name]
            if name in _CURSORS:
                return _CURSORS[name]
        while isinstance(value, ast.Await):
            value = value.value
        if isinstance(value, ast.Call):
            name = _qualified(_dotted(value.func), self.parsed)
            if name in _CONNECTIONS:
                return _CONNECTIONS[name]
            func = value.func
            if (
                isinstance(func, ast.Attribute)
                and func.attr == "cursor"
                and isinstance(func.value, ast.Name)
            ):
                return self.driver(func.value.id, scopes)
        return None

    def query(self, name: str, scopes: list[_Scope]) -> str | None:
        """The query literal ``name`` is bound to, in ``scopes``, when it has exactly one binding."""
        bindings = self._lookup(name, scopes)
        if bindings is None or len(bindings) != 1:
            return None
        return _literal(bindings[0][0])


def _literal(node: ast.expr | None) -> str | None:
    """The text of a string literal, possibly wrapped as ``SQL[T]("...")``."""
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Subscript) and len(node.args) == 1:
        if _dotted(node.func.value) in {"SQL", "sqlty.SQL"}:
            return _literal(node.args[0])
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    return None


def find_driver_calls(source: str | ParsedSource) -> list[HelperCall]:
    """Calls of ``execute``-like methods on a recognised connection or cursor, with their query and parameters.

    A call with no parameters has ``params == ()``, so a query that has placeholders is
    reported. ``executemany`` gives one entry per row of a list or tuple literal.
    """
    parsed = ParsedSource.of(source)
    if not any(driver in parsed.text for driver in ("psycopg", "sqlite3", "asyncpg")):
        return []
    if parsed.tree is None:
        return []
    scopes = _Scopes(parsed)
    found: list[HelperCall] = []
    aliases = parsed.typing_aliases
    for call in parsed.calls_to(None):
        func = call.func
        if not (
            isinstance(func, ast.Attribute)
            and isinstance(func.value, ast.Name)
            and (driver := scopes.driver(func.value.id, chain := scopes.chain(call)))
            and func.attr in _METHODS[driver]
            and call.args
            and not any(isinstance(arg, ast.Starred) for arg in call.args)
        ):
            continue
        first = call.args[0]
        query = _literal(first)
        if query is None and isinstance(first, ast.Name):
            query = scopes.query(first.id, chain)
        if query is None:
            continue
        stack = parsed.scopes_of(call)
        if driver == "asyncpg":
            if func.attr == "executemany":
                continue
            params: tuple[ParamArg, ...] | None = tuple(
                ParamArg(i, arg_type(arg, parsed, stack)) for i, arg in enumerate(call.args[1:], 1)
            )
            found.append(HelperCall(call.lineno, query, None, aliases, params))
            continue
        keyword = _PARAM_KEYWORD[driver]
        arg = call.args[1] if len(call.args) > 1 else None
        if arg is None:
            arg = next((k.value for k in call.keywords if k.arg == keyword), None)
            if arg is None and any(k.arg is None for k in call.keywords):
                continue
        if func.attr == "executemany":
            if isinstance(arg, (ast.List, ast.Tuple)):
                for row in arg.elts:
                    if (params := literal_params(row, parsed, stack)) is not None:
                        found.append(HelperCall(call.lineno, query, None, aliases, params))
            continue
        found.append(
            HelperCall(
                call.lineno,
                query,
                None,
                aliases,
                () if arg is None else literal_params(arg, parsed, stack),
            )
        )
    return found
