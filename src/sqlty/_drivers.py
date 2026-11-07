"""Driver calls that pass a query and its parameters: ``cursor.execute(query, params)``.

A receiver is recognised only when the file shows what it is: a variable bound to
``psycopg.connect(...)``, ``sqlite3.connect(...)`` or ``asyncpg.connect(...)`` (also as
``with ... as name``), to ``<connection>.cursor()``, or annotated with a driver class. The
query is a string literal, ``SQL[T]("...")`` of one, or a name assigned exactly one of
those. Anything else is skipped. The ``sqlty.drivers.psycopg`` helpers are psycopg calls.

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
import re
from collections import defaultdict

from sqlty._helpers import MARKER, HelperCall
from sqlty._params import SCALARS, ParamArg, arg_type, literal_params
from sqlty._source import ParsedSource, _fields

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
    "asyncpg": {"execute", "fetch", "fetchrow", "fetchval", "executemany", "fetchmany"},
}
_ANY_METHOD = frozenset().union(*_METHODS.values())
_METHOD_NAMES = re.compile(rf"\b(?:{'|'.join(sorted(_ANY_METHOD))})\b")
_ASYNCPG_MANY = frozenset({"executemany", "fetchmany"})
_PARAM_KEYWORD = {"psycopg": "params", "sqlite3": "parameters"}
# sqlite3 reads a str or bytes as a sequence, one parameter per item.
_SEQUENCE_SCALARS = {"psycopg": frozenset(), "sqlite3": frozenset({"str", "bytes"})}

# A t-string that starts with the marker.
_MARKED_TEMPLATE = re.compile(r"""(?<![\w])(?:[tT][rR]?|[rR][tT])(?:'|")+\s*--\s?sql""")
# psycopg's format specs that only pick the wire format of a value.
_WIRE_FORMATS = frozenset({None, "s", "b", "t"})


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
        pending = _children(scope)
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
            pending += _children(node)
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


def _children(node: ast.AST) -> list[ast.AST]:
    """The child nodes of ``node`` that can hold a binding: no names, constants or operators."""
    found: list[ast.AST] = []
    for field in _fields(type(node)):
        value = getattr(node, field)
        for child in value if type(value) is list else (value,):
            if child is not None and _fields(type(child)):
                found.append(child)
    return found


def _literal(node: ast.expr | None) -> str | None:
    """The text of a string literal, possibly wrapped as ``SQL[T]("...")``."""
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Subscript) and len(node.args) == 1:
        if _dotted(node.func.value) in {"SQL", "sqlty.SQL"}:
            return _literal(node.args[0])
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    return None


def find_driver_calls(source: str | ParsedSource, *, receivers: bool = True) -> list[HelperCall]:
    """Calls of ``execute``-like methods on a recognised connection or cursor, with their query and parameters.

    A call with no parameters has ``params == ()``, so a query that has placeholders is
    reported. ``executemany`` (and asyncpg's ``fetchmany``) gives one entry per row of a list or tuple literal.
    With ``receivers`` off (``driver-calls = false``) only the ``sqlty.drivers.psycopg`` helpers count.
    """
    parsed = ParsedSource.of(source)
    text = parsed.text
    if not any(driver in text for driver in ("psycopg", "sqlite3", "asyncpg")):
        return []
    if not any(method in text for method in ("execute", "fetch", "scalar")):
        return []
    if parsed.tree is None:
        return []
    helpers = any(
        f"{name}.".startswith(_BUNDLED_MODULE) or _BUNDLED_MODULE.startswith(f"{name}.")
        for name in parsed.imported_names.values()
    )
    if not (helpers or receivers):
        return []
    scopes = _Scopes(parsed)
    found: list[HelperCall] = []
    aliases = parsed.typing_aliases
    for call in parsed.calls_to(None if helpers else _METHOD_NAMES):
        if helpers and (bundled := _bundled(call, parsed)) is not None:
            for first, arg in bundled:
                query = _literal(first)
                if query is None and isinstance(first, ast.Name):
                    query = scopes.query(first.id, scopes.chain(call))
                found.extend(_calls(call, first, arg, "psycopg", parsed, aliases, query))
            continue
        func = call.func
        if not (
            receivers
            and isinstance(func, ast.Attribute)
            and func.attr in _ANY_METHOD
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
            if func.attr in _ASYNCPG_MANY:
                rows = call.args[1] if len(call.args) > 1 else None
                if rows is None:
                    rows = next((k.value for k in call.keywords if k.arg == "args"), None)
                if not isinstance(rows, (ast.List, ast.Tuple)):
                    continue
                arg_lists = [
                    row.elts
                    for row in rows.elts
                    if isinstance(row, (ast.List, ast.Tuple))
                    and not any(isinstance(item, ast.Starred) for item in row.elts)
                ]
            else:
                arg_lists = [call.args[1:]]
            found.extend(
                HelperCall(
                    call.lineno,
                    query,
                    None,
                    aliases,
                    tuple(
                        ParamArg(i, arg_type(arg, parsed, stack)) for i, arg in enumerate(args, 1)
                    ),
                    driver=driver,
                    query_node=first,
                )
                for args in arg_lists
            )
            continue
        keyword = _PARAM_KEYWORD[driver]
        arg = call.args[1] if len(call.args) > 1 else None
        if arg is None:
            arg = next((k.value for k in call.keywords if k.arg == keyword), None)
            if arg is None and any(k.arg is None for k in call.keywords):
                continue
        if func.attr != "executemany":
            found.extend(_calls(call, first, arg, driver, parsed, aliases, query))
        elif isinstance(arg, (ast.List, ast.Tuple)):
            for row in arg.elts:
                found.extend(_calls(call, first, row, driver, parsed, aliases, query, many=True))
    return found


def _calls(
    call: ast.Call,
    first: ast.expr,
    arg: ast.expr | None,
    driver: str,
    parsed: ParsedSource,
    aliases: dict[str, str],
    query: str | None,
    *,
    many: bool = False,
) -> list[HelperCall]:
    """``call`` running ``query`` with the parameters ``arg``, when they are known or a single value."""
    if query is None:
        return []
    stack = parsed.scopes_of(call)
    params = () if arg is None else literal_params(arg, parsed, stack)
    kind = arg_type(arg, parsed, stack) if arg is not None and params is None else None
    single = kind if kind in SCALARS - _SEQUENCE_SCALARS[driver] else None
    if many and params is None and single is None:
        return []
    return [
        HelperCall(
            call.lineno,
            query,
            None,
            aliases,
            params,
            driver=driver,
            single=single,
            query_node=first,
        )
    ]


_BUNDLED_MODULE = "sqlty.drivers.psycopg."
_BUNDLED = frozenset({f"{_BUNDLED_MODULE}execute", f"{_BUNDLED_MODULE}scalar"})


def _bundled(call: ast.Call, parsed: ParsedSource) -> list[tuple[ast.expr, ast.expr | None]] | None:
    """The query and parameters of a call to a ``sqlty.drivers.psycopg`` helper.

    ``None`` for any other call, and nothing when the call does not show its parameters.
    ``scalar`` takes a row type before the parameters, so a single argument after the query
    is the parameters only when it is a literal of them.
    """
    name = _qualified(_dotted(call.func), parsed)
    if name not in _BUNDLED:
        return None
    args = call.args
    keyword = next((k.value for k in call.keywords if k.arg == "params"), None)
    if (
        len(args) < 2
        or any(isinstance(arg, ast.Starred) for arg in args)
        or (keyword is None and any(k.arg is None for k in call.keywords))
    ):
        return []
    rest = args[2:]
    if name.endswith(".scalar") and len(rest) == 1:
        if isinstance(rest[0], ast.Name) and keyword is None:
            return []
        if not isinstance(rest[0], (ast.Tuple, ast.List, ast.Dict)):
            rest = []
    return [(args[1], rest[-1] if rest else keyword)]


def find_template_calls(source: str | ParsedSource) -> list[HelperCall]:
    """Each ``--sql`` t-string whose fields all pass values, as a call passing those values.

    psycopg sends a field ``{value}``, or one with the ``:s``, ``:b`` or ``:t`` spec, as a
    parameter, so field *k* binds placeholder *k* of the query. A t-string with any other
    field is left out: ``tstring-skipped`` reports it, or it inlines SQL.

    Example:
        >>> [(c.query, c.params) for c in find_template_calls('q = t"--sql\\\\nSELECT {1}, {x:b}"')]
        [('--sql\\nSELECT %s, %s', (ParamArg(key=1, type='int'), ParamArg(key=2, type=None)))]
    """
    parsed = ParsedSource.of(source)
    if not _MARKED_TEMPLATE.search(parsed.text) or parsed.tree is None:
        return []
    found: list[HelperCall] = []
    for template, stack in parsed.templates:
        parts: list[str] = []
        params: list[ParamArg] = []
        for value in template.values:
            if isinstance(value, ast.Constant):
                parts.append(str(value.value).replace("%", "%%"))
            elif (
                isinstance(value, ast.Interpolation)
                and value.conversion == -1
                and _spec(value) in _WIRE_FORMATS
            ):
                parts.append("%s")
                params.append(ParamArg(len(params) + 1, arg_type(value.value, parsed, stack)))
            else:
                break
        else:
            query = "".join(parts)
            if MARKER.match(query):
                found.append(
                    HelperCall(
                        template.lineno,
                        query,
                        None,
                        parsed.typing_aliases,
                        tuple(params),
                        driver="psycopg",
                        query_node=template,
                    )
                )
    return found


def _spec(field: ast.Interpolation) -> str | None:
    """The format spec of ``field`` when it is plain text."""
    match field.format_spec:
        case None:
            return None
        case ast.JoinedStr(values=[ast.Constant(value=str(spec))]):
            return spec
    return "{}"
