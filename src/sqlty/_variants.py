"""The queries a function builds along its paths, read from its source without running it.

A query assembled from names bound in branches has one variant per feasible path:

    if foo is None:
        cond = "foo IS NULL"
    else:
        cond = "foo = %s"
    fetch_all("SELECT id FROM foos WHERE " + cond, Row)

``query_variants`` walks the function around the call, forks at ``if``, ``match`` and
conditional expressions, drops the paths that ``return`` or ``raise``, and evaluates the
query on each path. Every part must be static: a string literal, ``+`` of static parts, a
name whose every binding is static, a ``Literal`` parameter, a lookup in a dict of
literals, the return of a function of the module built the same way, or a ``psycopg.sql``
composition of these. A name bound in a loop, a ``try``, a ``with`` or a walrus is not
followed, and neither is a module other than the call's own and those it imports
constants from.

Usage:
    found = query_variants(parsed, call, query, limit=64)
    if isinstance(found, Unchecked): ...  # dynamic, or over the limit
    else: [variant.query for variant in found]
"""

from __future__ import annotations

import ast
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, Final, NamedTuple

from sqlty._psycopg import Unrenderable

if TYPE_CHECKING:
    from sqlty._psycopg import Resolver
    from sqlty._source import ParsedSource


class Variant(NamedTuple):
    """One query a call can run, with the lines of the bindings that select it among the others."""

    query: str
    lines: tuple[int, ...]


class Unchecked(NamedTuple):
    """A query that is not checked: a part is not static, or it has too many variants.

    ``composed`` tells whether it is a ``psycopg.sql`` composition; ``reason`` is set when
    there are more variants than the limit.
    """

    composed: bool
    reason: str | None = None


PATH_LIMIT: Final = 4096
"""Paths followed at most, whatever the limit of variants: past it, the count is unknown."""

_DEPTH: Final = 64
_CALLS: Final = 8


def query_variants(
    parsed: ParsedSource,
    call: ast.Call,
    query: ast.expr,
    limit: int,
    resolve: Resolver | None = None,
) -> list[Variant] | Unchecked:
    """The queries ``query``, an argument of ``call``, takes on the paths reaching ``call``.

    ``resolve(source, dotted)`` finds the module-level value of a name imported from
    another module, as ``(its source, expression)``.

    Example:
        >>> from sqlty._source import ParsedSource
        >>> parsed = ParsedSource('''
        ... def find(flag: bool):
        ...     cond = "id = 1" if flag else "id = 2"
        ...     run("SELECT id FROM t WHERE " + cond)
        ... ''')
        >>> call = parsed.calls[0]
        >>> [v.query for v in query_variants(parsed, call, call.args[0], limit=64)]
        ['SELECT id FROM t WHERE id = 1', 'SELECT id FROM t WHERE id = 2']
    """
    walk = _Walk(resolve)
    stack = parsed.scopes_of(call)
    scope = stack[-1] if stack else parsed.tree
    if not isinstance(scope, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Module)):
        return Unchecked(False)
    try:
        frame = _Frame(parsed, scope, [query], stack[:-1])
        paths = walk.reach(frame, call)
        found: dict[str, frozenset[int]] = {}
        for path in paths:
            for value, lines, _ in walk.values(query, path, frame):
                found.setdefault(walk.render(value), lines)
    except _Dynamic:
        return Unchecked(walk.composed)
    except _TooMany:
        return Unchecked(
            walk.composed, f"over {PATH_LIMIT} variants, more than max-query-variants ({limit})"
        )
    if not found:
        return Unchecked(walk.composed)
    if len(found) > limit:
        return Unchecked(
            walk.composed, f"{len(found)} variants, more than max-query-variants ({limit})"
        )
    shared = frozenset.intersection(*found.values())
    return [
        Variant(query, tuple(sorted(lines - shared or lines))) for query, lines in found.items()
    ]


class _Dynamic(Exception):
    """A part that is not static."""


class _TooMany(Exception):
    """More paths than ``PATH_LIMIT``."""


class _Reached(Exception):
    """The walk reached the call, along ``paths``."""

    def __init__(self, paths: list[_Path]) -> None:
        super().__init__()
        self.paths = paths


@dataclass(frozen=True)
class _Bound:
    value: Any
    lines: frozenset[int]


_UNKNOWN: Final = _Bound(None, frozenset())
"""The value of a name bound to something that is not static."""


@dataclass
class _Path:
    """The static names bound so far on one path, and the conditions it took."""

    env: dict[str, _Bound] = field(default_factory=dict)
    conds: dict[str, tuple[bool, frozenset[str]]] = field(default_factory=dict)

    def bind(self, name: str, bound: _Bound) -> _Path:
        return _Path({**self.env, name: bound}, self._kept({name}))

    def assume(self, test: ast.expr, outcome: bool) -> _Path:
        return _Path(self.env, {**self.conds, ast.dump(test): (outcome, _names(test))})

    def forget(self, names: set[str]) -> _Path:
        return _Path(self.env, self._kept(names)) if names else self

    def _kept(self, names: set[str]) -> dict[str, tuple[bool, frozenset[str]]]:
        return {key: cond for key, cond in self.conds.items() if not cond[1] & names}

    def key(self) -> tuple[tuple[str, int, str, frozenset[int]], ...]:
        return tuple(
            sorted((name, b is _UNKNOWN, repr(b.value), b.lines) for name, b in self.env.items())
        )


_Function = ast.FunctionDef | ast.AsyncFunctionDef
_NESTED = (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef, ast.Lambda)
_COMPREHENSIONS = (ast.ListComp, ast.SetComp, ast.DictComp, ast.GeneratorExp)
_GUARDED = (ast.For, ast.AsyncFor, ast.While, ast.Try, ast.TryStar, ast.With, ast.AsyncWith)
_EXITS = (ast.Return, ast.Raise, ast.Break, ast.Continue)


def _names(node: ast.AST) -> frozenset[str]:
    return frozenset(n.id for n in ast.walk(node) if isinstance(n, ast.Name))


def _own_nodes(nodes: list[ast.stmt]) -> list[ast.AST]:
    """The nodes of ``nodes``, without the bodies of nested functions, classes or comprehensions."""
    found: list[ast.AST] = []
    todo: list[ast.AST] = list(nodes)
    while todo:
        node = todo.pop()
        found.append(node)
        if isinstance(node, (*_NESTED, *_COMPREHENSIONS)):
            continue
        todo.extend(ast.iter_child_nodes(node))
    return found


def _contains(outer: ast.stmt, inner: ast.expr) -> bool:
    start = (outer.lineno, outer.col_offset)
    end = (outer.end_lineno or 0, outer.end_col_offset or 0)
    return (
        start <= (inner.lineno, inner.col_offset)
        and (
            inner.end_lineno or 0,
            inner.end_col_offset or 0,
        )
        <= end
    )


class _Frame:
    """A function (or the module) whose bindings a walk follows.

    ``local`` are the names it binds, ``relevant`` those the query reads, directly or
    through the values bound to other names. A relevant name bound other than by a plain
    assignment outside loops and ``try`` makes the query dynamic. A frame without a
    scope reads module-level constants only.
    """

    def __init__(
        self,
        parsed: ParsedSource,
        scope: _Function | ast.Module | None,
        roots: list[ast.expr],
        outer: tuple[ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef, ...] = (),
    ) -> None:
        self.parsed = parsed
        self.body: list[ast.stmt] = [] if scope is None else scope.body
        self.params: dict[str, ast.arg] = {}
        if isinstance(scope, (ast.FunctionDef, ast.AsyncFunctionDef)):
            args = scope.args
            every = [*args.posonlyargs, *args.args, *args.kwonlyargs, args.vararg, args.kwarg]
            self.params = {a.arg: a for a in every if a is not None}
        self.outer: set[str] = set()
        for enclosing in outer:
            if not isinstance(enclosing, ast.ClassDef):
                self.outer |= _bound_names(enclosing.body)
                self.outer.update(a.arg for a in ast.walk(enclosing.args) if isinstance(a, ast.arg))
        nodes = _own_nodes(self.body)
        guarded = {
            id(n) for g in nodes if isinstance(g, _GUARDED) for n in _own_nodes(_guarded_body(g))
        }
        values: dict[str, list[ast.expr]] = {}
        followed: set[int] = set()
        for node in nodes:
            if id(node) in guarded:
                continue
            if isinstance(node, ast.AnnAssign) and node.value is None:
                followed.add(id(node.target))
            for target, value in _assignments(node):
                followed.add(id(target))
                values.setdefault(target.id, []).append(value)
        stores = _stores(nodes)
        other = {name for name, store in stores if store not in followed}
        self.local = {name for name, _ in stores}
        # A function of the module is called by name; one defined in a function is not followed.
        self.shadowed = (
            _defined(self.body)
            if isinstance(scope, (ast.FunctionDef, ast.AsyncFunctionDef))
            else set()
        )
        self.relevant = _closure(roots, values)
        if self.relevant & other:
            raise _Dynamic
        self.returns_guarded = any(isinstance(n, ast.Return) and id(n) in guarded for n in nodes)


def _guarded_body(
    node: ast.For | ast.AsyncFor | ast.While | ast.Try | ast.TryStar | ast.With | ast.AsyncWith,
) -> list[ast.stmt]:
    """The statements of a loop, ``try`` or ``with``, which may run any number of times or partly."""
    if isinstance(node, (ast.With, ast.AsyncWith)):
        return node.body
    if isinstance(node, (ast.Try, ast.TryStar)):
        return [
            *node.body,
            *(s for h in node.handlers for s in h.body),
            *node.orelse,
            *node.finalbody,
        ]
    return [*node.body, *node.orelse]


def _assignments(node: ast.AST) -> list[tuple[ast.Name, ast.expr]]:
    """The names a plain assignment binds, with the value each takes."""
    match node:
        case ast.Assign(targets=targets, value=value):
            found: list[tuple[ast.Name, ast.expr]] = []
            for target in targets:
                if isinstance(target, ast.Name):
                    found.append((target, value))
                elif (
                    isinstance(target, (ast.Tuple, ast.List))
                    and isinstance(value, (ast.Tuple, ast.List))
                    and len(target.elts) == len(value.elts)
                ):
                    found.extend(
                        (t, v)
                        for t, v in zip(target.elts, value.elts, strict=True)
                        if isinstance(t, ast.Name)
                    )
            return found
        case ast.AnnAssign(target=ast.Name() as target, value=value) if value is not None:
            return [(target, value)]
        case ast.AugAssign(target=ast.Name() as target, op=ast.Add(), value=value):
            return [(target, value)]
    return []


def _stores(nodes: list[ast.AST]) -> list[tuple[str, int | None]]:
    """Every name ``nodes`` bind, with the ``ast.Name`` that binds it, if any."""
    found: list[tuple[str, int | None]] = []
    for node in nodes:
        match node:
            case ast.Name(ctx=ast.Store() | ast.Del()):
                found.append((node.id, id(node)))
            case (
                ast.ExceptHandler(name=str() as name)
                | ast.MatchAs(name=str() as name)
                | ast.MatchStar(name=str() as name)
                | ast.MatchMapping(rest=str() as name)
            ):
                found.append((name, None))
            case ast.Global(names=names) | ast.Nonlocal(names=names):
                found.extend((name, None) for name in names)
    return found


def _defined(body: list[ast.stmt]) -> set[str]:
    """Names ``body`` binds with ``def``, ``class`` or ``import``, which a walk does not track."""
    names: set[str] = set()
    for node in _own_nodes(body):
        match node:
            case (
                ast.FunctionDef(name=name)
                | ast.AsyncFunctionDef(name=name)
                | ast.ClassDef(name=name)
            ):
                names.add(name)
            case ast.Import(names=aliases) | ast.ImportFrom(names=aliases):
                names.update((a.asname or a.name).partition(".")[0] for a in aliases)
    return names


def _bound_names(body: list[ast.stmt]) -> set[str]:
    return {name for name, _ in _stores(_own_nodes(body))} | _defined(body)


def _closure(roots: list[ast.expr], values: dict[str, list[ast.expr]]) -> set[str]:
    """The names ``roots`` read, and those the values bound to them read, transitively."""
    seen: set[str] = set()
    todo = [name for root in roots for name in _names(root)]
    while todo:
        if (name := todo.pop()) in seen:
            continue
        seen.add(name)
        todo.extend(n for value in values.get(name, []) for n in _names(value))
    return seen


class _Walk:
    """Paths through function bodies, and the values of expressions along them."""

    def __init__(self, resolve: Resolver | None) -> None:
        self.resolve = resolve
        self.composed = False
        self.depth = 0
        self.calls = 0
        self._touches: dict[int, bool] = {}
        self._constants: dict[int, _Frame] = {}

    def constants(self, parsed: ParsedSource) -> _Frame:
        """The frame reading the module-level constants of ``parsed``."""
        if (frame := self._constants.get(id(parsed))) is None:
            frame = self._constants[id(parsed)] = _Frame(parsed, None, [])
        return frame

    def reach(self, frame: _Frame, target: ast.expr) -> list[_Path]:
        """The paths from the start of ``frame`` to ``target``."""
        try:
            self.block(frame, frame.body, [_Path()], target, None, branch=False)
        except _Reached as reached:
            return reached.paths
        return []

    def render(self, value: Any) -> str:
        """The SQL of a value: a string, or a ``psycopg.sql`` composition."""
        if isinstance(value, str):
            return value
        try:
            return str(value.as_string(None))
        except AttributeError, TypeError, ValueError, NotImplementedError:
            raise _Dynamic from None

    def block(
        self,
        frame: _Frame,
        body: list[ast.stmt],
        paths: list[_Path],
        target: ast.expr | None,
        returns: list[tuple[Any, frozenset[int]]] | None,
        branch: bool,
    ) -> list[_Path]:
        """The paths that run through ``body`` to its end; ``returns`` collects what returns."""
        for stmt in body:
            if target is not None and _contains(stmt, target):
                self.enter(frame, stmt, paths, target, returns)
            paths = self.statement(frame, stmt, paths, returns, branch)
            if len(paths) > PATH_LIMIT:
                raise _TooMany
        return paths

    def enter(
        self,
        frame: _Frame,
        stmt: ast.stmt,
        paths: list[_Path],
        target: ast.expr,
        returns: list[tuple[Any, frozenset[int]]] | None,
    ) -> None:
        """Walk into the block of ``stmt`` holding ``target``; raises ``_Reached``."""
        blocks: list[tuple[list[ast.stmt], list[_Path]]] = []
        match stmt:
            case ast.If(test=test, body=body, orelse=orelse):
                blocks = [(body, _assume(paths, test, True)), (orelse, _assume(paths, test, False))]
            case ast.For() | ast.AsyncFor() | ast.While():
                blocks = [(stmt.body, paths), (stmt.orelse, paths)]
            case ast.With() | ast.AsyncWith():
                blocks = [(stmt.body, paths)]
            case ast.Try() | ast.TryStar():
                handlers = [(h.body, paths) for h in stmt.handlers]
                blocks = [
                    (stmt.body, paths),
                    *handlers,
                    (stmt.orelse, paths),
                    (stmt.finalbody, paths),
                ]
            case ast.Match():
                blocks = self.cases(frame, stmt, paths)
        for body, entry in blocks:
            if any(_contains(s, target) for s in body):
                self.block(
                    frame,
                    body,
                    entry,
                    target,
                    returns,
                    branch=isinstance(stmt, (ast.If, ast.Match)),
                )
        raise _Reached(paths)

    def statement(
        self,
        frame: _Frame,
        stmt: ast.stmt,
        paths: list[_Path],
        returns: list[tuple[Any, frozenset[int]]] | None,
        branch: bool,
    ) -> list[_Path]:
        match stmt:
            case ast.Assign() | ast.AnnAssign() | ast.AugAssign():
                return self.assign(frame, stmt, paths, branch)
            case ast.If(test=test, body=body, orelse=orelse):
                if not self.touches(frame, stmt):
                    return [path.forget(_bound_names([stmt])) for path in paths]
                taken = self.block(frame, body, _assume(paths, test, True), None, returns, True)
                other = self.block(frame, orelse, _assume(paths, test, False), None, returns, True)
                return _dedupe([*taken, *other])
            case ast.Match():
                if not self.touches(frame, stmt):
                    return [path.forget(_bound_names([stmt])) for path in paths]
                found = [
                    p
                    for body, entry in self.cases(frame, stmt, paths)
                    for p in self.block(frame, body, entry, None, returns, True)
                ]
                return _dedupe(found)
            case ast.Return(value=value) if returns is not None:
                for path in paths:
                    if value is None:
                        raise _Dynamic
                    returns.extend(
                        (found, lines | {stmt.lineno})
                        for found, lines, _ in self.values(value, path, frame)
                    )
                return []
            case ast.Return() | ast.Raise() | ast.Break() | ast.Continue():
                return []
        return [path.forget(_bound_names([stmt])) for path in paths]

    def cases(
        self, frame: _Frame, stmt: ast.Match, paths: list[_Path]
    ) -> list[tuple[list[ast.stmt], list[_Path]]]:
        """The paths entering each case of ``stmt``, then those that match none, with an empty body.

        A case of string literals on a name keeps the paths where the name takes one of them.
        """
        blocks: list[tuple[list[ast.stmt], list[_Path]]] = []
        for case in stmt.cases:
            strings = _strings(case.pattern)
            if case.guard is not None or strings is None or not isinstance(stmt.subject, ast.Name):
                blocks.append((case.body, paths))
                if _irrefutable(case):
                    return blocks
                continue
            entry: list[_Path] = []
            rest: list[_Path] = []
            for path in paths:
                try:
                    found = self.name(stmt.subject, path, frame)
                except _Dynamic:
                    entry.append(path)
                    rest.append(path)
                    continue
                for value, _, after in found:
                    (entry if isinstance(value, str) and value in strings else rest).append(after)
            blocks.append((case.body, entry))
            paths = rest
        blocks.append(([], paths))
        return blocks

    def touches(self, frame: _Frame, stmt: ast.stmt) -> bool:
        """Whether ``stmt`` binds a name the query reads, or may leave its block."""
        if (found := self._touches.get(id(stmt))) is None:
            found = any(
                isinstance(node, _EXITS)
                or any(target.id in frame.relevant for target, _ in _assignments(node))
                for node in _own_nodes([stmt])
            )
            self._touches[id(stmt)] = found
        return found

    def assign(
        self,
        frame: _Frame,
        stmt: ast.Assign | ast.AnnAssign | ast.AugAssign,
        paths: list[_Path],
        branch: bool,
    ) -> list[_Path]:
        names = _bound_names([stmt])
        tracked = [(t.id, value) for t, value in _assignments(stmt) if t.id in frame.relevant]
        if not tracked:
            return [path.forget(names) for path in paths]
        line = frozenset({stmt.lineno}) if branch else frozenset()
        found: list[_Path] = []
        for path in paths:
            # Every value is read before any name is bound.
            options: list[tuple[dict[str, _Bound], _Path]] = [({}, path)]
            for name, value in tracked:
                if isinstance(stmt, ast.AugAssign):
                    value = ast.BinOp(ast.Name(name, ast.Load()), ast.Add(), value)
                    ast.copy_location(value, stmt)
                next_options = []
                for bindings, current in options:
                    try:
                        alternatives = self.values(value, current, frame)
                    except _Dynamic:
                        next_options.append(({**bindings, name: _UNKNOWN}, current))
                        continue
                    for result, lines, after in alternatives:
                        next_options.append(
                            ({**bindings, name: _Bound(result, lines | line)}, after)
                        )
                options = next_options
            for bindings, current in options:
                current = current.forget(names - bindings.keys())
                for name, binding in bindings.items():
                    current = current.bind(name, binding)
                found.append(current)
        return _dedupe(found)

    def values(
        self, node: ast.expr, path: _Path, frame: _Frame
    ) -> list[tuple[Any, frozenset[int], _Path]]:
        """The values ``node`` takes on ``path``, each with its lines and the path it leaves.

        An empty list when the path cannot get there: it reads an unbound name or a missing key.
        """
        self.depth += 1
        try:
            if self.depth > _DEPTH:
                raise _Dynamic
            return self._values(node, path, frame)
        finally:
            self.depth -= 1

    def _values(
        self, node: ast.expr, path: _Path, frame: _Frame
    ) -> list[tuple[Any, frozenset[int], _Path]]:
        none: frozenset[int] = frozenset()
        match node:
            case ast.Constant(value=str() as value):
                return [(value, none, path)]
            case ast.Name():
                return self.name(node, path, frame)
            case ast.BinOp(left=left, op=ast.Add(), right=right):
                found = []
                for a, a_lines, middle in self.values(left, path, frame):
                    for b, b_lines, end in self.values(right, middle, frame):
                        try:
                            found.append((a + b, a_lines | b_lines, end))
                        except TypeError:  # a dict, or a string and a composition
                            raise _Dynamic from None
                return found
            case ast.IfExp(test=test, body=body, orelse=orelse):
                found = []
                for branch, outcome in ((body, True), (orelse, False)):
                    for taken in _assume([path], test, outcome):
                        found.extend(
                            (value, lines | {branch.lineno}, end)
                            for value, lines, end in self.values(branch, taken, frame)
                        )
                return found
            case ast.Subscript(value=container, slice=key) if not isinstance(key, ast.Slice):
                found = []
                for whole, w_lines, middle in self.values(container, path, frame):
                    if not isinstance(whole, dict):
                        raise _Dynamic
                    for index, k_lines, end in self.values(key, middle, frame):
                        if index in whole:
                            found.append((whole[index], w_lines | k_lines, end))
                return found
            case ast.Dict(keys=keys, values=items) if all(k is not None for k in keys):
                built = {}
                for k, v in zip(keys, items, strict=True):
                    assert k is not None
                    built[self.single(k, path, frame)] = self.single(v, path, frame)
                return [(built, none, path)]
            case ast.Attribute():
                return self.imported(node, path, frame)
            case ast.Call():
                return self.call(node, path, frame)
        raise _Dynamic

    def single(self, node: ast.expr, path: _Path, frame: _Frame) -> Any:
        found = self.values(node, path, frame)
        if len(found) != 1:
            raise _Dynamic
        return found[0][0]

    def name(
        self, node: ast.Name, path: _Path, frame: _Frame
    ) -> list[tuple[Any, frozenset[int], _Path]]:
        name = node.id
        if name in frame.local or name in frame.params:
            if (bound := path.env.get(name)) is not None:
                if bound is _UNKNOWN:
                    raise _Dynamic
                return [(bound.value, bound.lines, path)]
            if name not in frame.params:
                return []
            arg = frame.params[name]
            if (members := _literal_members(arg.annotation)) is None:
                raise _Dynamic
            return [
                (
                    member,
                    frozenset({arg.lineno}),
                    path.bind(name, _Bound(member, frozenset({arg.lineno}))),
                )
                for member in members
            ]
        if name in frame.outer or name in frame.shadowed:
            raise _Dynamic
        return self.imported(node, path, frame)

    def imported(
        self, node: ast.expr, path: _Path, frame: _Frame
    ) -> list[tuple[Any, frozenset[int], _Path]]:
        """The value of a module-level name, of this module or one it imports from."""
        parsed = frame.parsed
        if isinstance(node, ast.Name) and (value := parsed.constants.get(node.id)) is not None:
            return self.values(value, path, self.constants(parsed))
        dotted = _dotted(node)
        if (
            dotted is None
            or self.resolve is None
            or (found := self.resolve(parsed, dotted)) is None
        ):
            raise _Dynamic
        source, value = found
        return self.values(value, path, self.constants(source))

    def call(
        self, node: ast.Call, path: _Path, frame: _Frame
    ) -> list[tuple[Any, frozenset[int], _Path]]:
        func = node.func
        if isinstance(func, ast.Name) and func.id not in (
            frame.local | frame.params.keys() | frame.outer | frame.shadowed
        ):
            defined = [f for f in frame.parsed.functions if f.name == func.id]
            if len(defined) == 1 and not defined[0].decorator_list:
                return self.returns(defined[0], node, path, frame)
        if "psycopg" not in frame.parsed.text:
            raise _Dynamic
        paths = [path]
        for param in sorted(
            {n.id for n in ast.walk(node) if isinstance(n, ast.Name)} & frame.params.keys()
        ):
            paths = [after for p in paths for _, _, after in self.name(ast.Name(param), p, frame)]
        return [self.compose(node, p, frame) for p in paths]

    def compose(
        self, node: ast.Call, path: _Path, frame: _Frame
    ) -> tuple[Any, frozenset[int], _Path]:
        read: set[int] = set()

        def lookup(name: ast.Name) -> Any:
            found = self.name(name, path, frame)
            if len(found) != 1:
                raise _Dynamic
            read.update(found[0][1])
            return found[0][0]

        try:
            value = frame.parsed.psycopg.compose(node, lookup, self.resolve)
        except Unrenderable as error:
            self.composed = self.composed or error.composed
            raise _Dynamic from None
        self.composed = True
        return (value, frozenset(read), path)

    def returns(
        self, function: _Function, node: ast.Call, path: _Path, frame: _Frame
    ) -> list[tuple[Any, frozenset[int], _Path]]:
        """What ``function`` returns when ``node`` calls it, with the arguments it passes."""
        if self.calls >= _CALLS or any(isinstance(a, ast.Starred) for a in node.args):
            raise _Dynamic
        roots = [
            r.value for r in _own_nodes(function.body) if isinstance(r, ast.Return) and r.value
        ]
        callee = _Frame(frame.parsed, function, roots)
        if callee.returns_guarded:
            raise _Dynamic
        args = function.args
        positional = [*args.posonlyargs, *args.args]
        defaults = {
            **{a.arg: d for a, d in zip(positional[::-1], args.defaults[::-1], strict=False)},
            **{
                a.arg: d
                for a, d in zip(args.kwonlyargs, args.kw_defaults, strict=True)
                if d is not None
            },
        }
        passed = {a.arg: v for a, v in zip(positional, node.args, strict=False)}
        passed.update((k.arg, k.value) for k in node.keywords if k.arg is not None)
        # Each entry binds the arguments of one call, with their lines and the caller's path after.
        entries = [(_Path(), frozenset[int](), path)]
        for name in sorted(callee.relevant & callee.params.keys()):
            literal = _literal_members(callee.params[name].annotation) is not None
            if name not in passed and name not in defaults:
                if literal:
                    continue
                raise _Dynamic
            forked: list[tuple[_Path, frozenset[int], _Path]] = []
            for entry, lines, caller in entries:
                try:
                    if name in passed:
                        found = self.values(passed[name], caller, frame)
                    else:
                        found = [
                            (v, n, caller)
                            for v, n, _ in self.values(
                                defaults[name], _Path(), self.constants(frame.parsed)
                            )
                        ]
                except _Dynamic:
                    forked.append((entry if literal else entry.bind(name, _UNKNOWN), lines, caller))
                    continue
                forked.extend(
                    (entry.bind(name, _Bound(v, frozenset())), lines | v_lines, after)
                    for v, v_lines, after in found
                )
            entries = forked
        results: list[tuple[Any, frozenset[int], _Path]] = []
        self.calls += 1
        try:
            for entry, lines, caller in entries:
                collected: list[tuple[Any, frozenset[int]]] = []
                if self.block(callee, callee.body, [entry], None, collected, branch=False):
                    raise _Dynamic
                results.extend((value, lines | v_lines, caller) for value, v_lines in collected)
        finally:
            self.calls -= 1
        return results


def _assume(paths: list[_Path], test: ast.expr, outcome: bool) -> list[_Path]:
    """The paths that can take ``outcome`` of ``test``: one tested before must agree."""
    key = ast.dump(test)
    return [
        path if key in path.conds else path.assume(test, outcome)
        for path in paths
        if path.conds.get(key, (outcome,))[0] == outcome
    ]


def _dedupe(paths: list[_Path]) -> list[_Path]:
    """One path per set of bindings, keeping the conditions every merged path agrees on."""
    found: dict[tuple[Any, ...], _Path] = {}
    for path in paths:
        key = path.key()
        if (kept := found.get(key)) is None:
            found[key] = path
        else:
            found[key] = _Path(
                kept.env, {k: c for k, c in kept.conds.items() if path.conds.get(k) == c}
            )
    return list(found.values())


def _strings(pattern: ast.pattern) -> set[str] | None:
    """The strings a pattern of string literals matches, ``None`` for any other pattern."""
    match pattern:
        case ast.MatchValue(value=ast.Constant(value=str() as value)):
            return {value}
        case ast.MatchOr(patterns=patterns):
            found: set[str] = set()
            for alternative in patterns:
                if (strings := _strings(alternative)) is None:
                    return None
                found |= strings
            return found
    return None


def _irrefutable(case: ast.match_case) -> bool:
    return (
        case.guard is None
        and isinstance(case.pattern, ast.MatchAs)
        and case.pattern.pattern is None
    )


def _literal_members(annotation: ast.expr | None) -> list[str] | None:
    """The strings of a ``Literal[...]`` annotation, or ``None`` for any other."""
    if isinstance(annotation, ast.Constant) and isinstance(annotation.value, str):
        try:
            annotation = ast.parse(annotation.value, mode="eval").body
        except SyntaxError:
            annotation = None
    if (
        isinstance(annotation, ast.Subscript)
        and (_dotted(annotation.value) or "").rpartition(".")[2] == "Literal"
    ):
        items = (
            annotation.slice.elts if isinstance(annotation.slice, ast.Tuple) else [annotation.slice]
        )
        members = [
            i.value for i in items if isinstance(i, ast.Constant) and isinstance(i.value, str)
        ]
        if len(members) == len(items):
            return members
    return None


def _dotted(node: ast.expr) -> str | None:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute) and (base := _dotted(node.value)):
        return f"{base}.{node.attr}"
    return None
