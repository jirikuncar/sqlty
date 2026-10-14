"""Driver calls that pass a query and its parameters: ``cursor.execute(query, params)``.

A receiver is recognised only when the file shows what it is: a variable bound to
``psycopg.connect(...)``, ``sqlite3.connect(...)`` or ``asyncpg.connect(...)`` (also as
``with ... as name``), to ``<connection>.cursor()``, or annotated with a driver class. The
query is a string literal, ``SQL[T]("...")`` of one, or a name assigned exactly one of
those. Anything else is skipped.

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


def _receivers(parsed: ParsedSource) -> dict[str, str]:
    """Names the file binds to a connection or cursor, to the driver; ambiguous names are dropped."""
    tree = parsed.tree
    if tree is None:
        return {}
    kinds: dict[str, set[str | None]] = defaultdict(set)

    def kind_of(value: ast.expr | None, annotation: ast.expr | None = None) -> str | None:
        if annotation is not None:
            name = _qualified(_dotted(annotation), parsed)
            if name in _CONNECTIONS or name in _CURSORS:
                return (_CONNECTIONS | _CURSORS)[name]
        while isinstance(value, ast.Await):
            value = value.value
        if isinstance(value, ast.Call):
            name = _qualified(_dotted(value.func), parsed)
            if name in _CONNECTIONS:
                return _CONNECTIONS[name]
            func = value.func
            if isinstance(func, ast.Attribute) and func.attr == "cursor":
                if isinstance(func.value, ast.Name) and func.value.id in kinds:
                    return (
                        next(iter(kinds[func.value.id])) if len(kinds[func.value.id]) == 1 else None
                    )
        return None

    for node in ast.walk(tree):
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            if isinstance(node.targets[0], ast.Name):
                kinds[node.targets[0].id].add(kind_of(node.value))
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            kinds[node.target.id].add(kind_of(node.value, node.annotation))
        elif isinstance(node, (ast.With, ast.AsyncWith)):
            for item in node.items:
                if isinstance(item.optional_vars, ast.Name):
                    kinds[item.optional_vars.id].add(kind_of(item.context_expr))
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            for arg in [*node.args.posonlyargs, *node.args.args, *node.args.kwonlyargs]:
                kinds[arg.arg].add(kind_of(None, arg.annotation))
    resolved: dict[str, str] = {}
    for name, found in kinds.items():
        driver = next(iter(found)) if len(found) == 1 else None
        if driver is not None:
            resolved[name] = driver
    return resolved


def _literals(parsed: ParsedSource) -> dict[str, str]:
    """Names assigned exactly once, to a query literal."""
    tree = parsed.tree
    assert tree is not None
    values: dict[str, list[str | None]] = defaultdict(list)
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            if isinstance(node.targets[0], ast.Name):
                values[node.targets[0].id].append(_literal(node.value))
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            values[node.target.id].append(_literal(node.value))
    return {n: v[0] for n, v in values.items() if len(v) == 1 and v[0] is not None}


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
    receivers = _receivers(parsed)
    if not receivers:
        return []
    literals = _literals(parsed)
    found: list[HelperCall] = []
    aliases = parsed.typing_aliases
    for call in parsed.calls_to(None):
        func = call.func
        if not (
            isinstance(func, ast.Attribute)
            and isinstance(func.value, ast.Name)
            and (driver := receivers.get(func.value.id))
            and func.attr in _METHODS[driver]
            and call.args
            and not any(isinstance(arg, ast.Starred) for arg in call.args)
        ):
            continue
        first = call.args[0]
        query = _literal(first)
        if query is None and isinstance(first, ast.Name):
            query = literals.get(first.id)
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
