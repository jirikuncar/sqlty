"""Helpers whose query parameter is marked ``Annotated[LiteralString, SQL]``.

A call passing a string literal to such a parameter is a checked query, with
no ``--sql`` marker or ``SQL[T]`` wrapper. The row type is the argument bound
to the parameter annotated ``type[T]`` / ``TypeForm[T]`` where ``T`` also
appears in the return annotation.

Usage:
    index = HelperIndex()
    index.add("pkg.queries", source_of_pkg_queries)
    find_helper_calls(source, index, "pkg.app")
"""

from __future__ import annotations

import ast
import re
from dataclasses import dataclass
from typing import NamedTuple

from sqlty._source import ParsedSource

_TYPE_FORMS = frozenset({"type", "Type", "TypeForm"})


@dataclass(frozen=True)
class Helper:
    """A function with a marked query parameter.

    ``positional`` lists the parameters a call may bind by position, ``keywords``
    those it may bind by name. ``row`` is ``None`` when no ``type[T]`` parameter
    shares ``T`` with the return type.
    """

    name: str
    positional: tuple[str, ...]
    keywords: frozenset[str]
    query: str
    row: str | None


class HelperCall(NamedTuple):
    """A call passing a string literal to a helper's query parameter."""

    line: int
    query: str
    row: ast.expr | None
    typing_aliases: dict[str, str]


def _dotted(node: ast.expr) -> str | None:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute) and (base := _dotted(node.value)):
        return f"{base}.{node.attr}"
    return None


def _expr(annotation: ast.expr | None) -> ast.expr | None:
    """The annotation, with a quoted one parsed."""
    if isinstance(annotation, ast.Constant) and isinstance(annotation.value, str):
        try:
            return ast.parse(annotation.value, mode="eval").body
        except SyntaxError:
            return None
    return annotation


def _is_marked(annotation: ast.expr | None, annotated: set[str], marker: set[str]) -> bool:
    if (annotation := _expr(annotation)) is None:
        return False
    for node in ast.walk(annotation):
        if (
            isinstance(node, ast.Subscript)
            and _dotted(node.value) in annotated
            and isinstance(node.slice, ast.Tuple)
        ):
            if any(_dotted(meta) in marker for meta in node.slice.elts[1:]):
                return True
    return False


def _row_param(args: ast.arguments, returns: ast.expr | None) -> str | None:
    """The first parameter annotated ``type[T]`` / ``TypeForm[T]`` with ``T`` in ``returns``."""
    if (returns := _expr(returns)) is None:
        return None
    returned = {n.id for n in ast.walk(returns) if isinstance(n, ast.Name)}
    for arg in [*args.posonlyargs, *args.args, *args.kwonlyargs]:
        if (annotation := _expr(arg.annotation)) is None:
            continue
        for node in ast.walk(annotation):
            if (
                isinstance(node, ast.Subscript)
                and (name := _dotted(node.value))
                and name.rpartition(".")[2] in _TYPE_FORMS
                and isinstance(node.slice, ast.Name)
                and node.slice.id in returned
            ):
                return arg.arg
    return None


def index_functions(source: str | ParsedSource) -> dict[str, Helper]:
    """Module-level functions with a marked query parameter, by name.

    Methods are not indexed.

    Example:
        >>> src = "from typing import Annotated\\nfrom sqlty import SQL\\ndef f[T](q: Annotated[str, SQL], t: type[T], /) -> T: ..."
        >>> index_functions(src)["f"]
        Helper(name='f', positional=('q', 't'), keywords=frozenset(), query='q', row='t')
    """
    parsed = ParsedSource.of(source)
    annotated, marker = parsed.marker_names
    if not annotated or not marker:
        return {}
    found: dict[str, Helper] = {}
    for node in parsed.functions:
        args = node.args
        query = next(
            (
                a.arg
                for a in [*args.posonlyargs, *args.args, *args.kwonlyargs]
                if _is_marked(a.annotation, annotated, marker)
            ),
            None,
        )
        if query is None:
            continue
        found[node.name] = Helper(
            name=node.name,
            positional=tuple(a.arg for a in [*args.posonlyargs, *args.args]),
            keywords=frozenset(a.arg for a in [*args.args, *args.kwonlyargs]),
            query=query,
            row=_row_param(args, node.returns),
        )
    return found


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
        self.modules: dict[str, dict[str, Helper]] = {}
        self._pattern: re.Pattern[str] | None = None

    def add(self, module: str, source: str | ParsedSource) -> bool:
        """Index the helpers of ``source`` as module ``module``; whether it has any."""
        parsed = ParsedSource.of(source)
        if not parsed.may_define_helpers or not (found := index_functions(parsed)):
            return False
        self.modules.setdefault(module, {}).update(found)
        self._pattern = None
        return True

    def mentions(self, source: str) -> bool:
        """Whether ``source`` contains the name of some helper, so it may call one."""
        if self._pattern is None:
            names = {name for helpers in self.modules.values() for name in helpers}
            self._pattern = re.compile("|".join(map(re.escape, sorted(names))) or r"(?!)")
        return self._pattern.search(source) is not None


def _imports(
    parsed: ParsedSource, module: str | None
) -> tuple[dict[str, tuple[str, str]], dict[str, str]]:
    """Local names bound to imported functions ``(module, name)`` and to modules.

    Example:
        >>> _imports(ParsedSource("from a.b import f as g\\nimport a.c as m"), None)
        ({'g': ('a.b', 'f')}, {'g': 'a.b.f', 'm': 'a.c'})
    """
    functions: dict[str, tuple[str, str]] = {}
    modules: dict[str, str] = {}
    package = module.rpartition(".")[0] if module else ""
    for node in parsed.imports:
        if isinstance(node, ast.Import):
            for alias in node.names:
                modules[alias.asname or alias.name] = alias.name
        elif isinstance(node, ast.ImportFrom):
            base = node.module or ""
            if node.level:
                if module is None:
                    continue
                parts = package.split(".") if package else []
                if node.level - 1 > len(parts):
                    continue
                parts = parts[: len(parts) - (node.level - 1)]
                base = ".".join([*parts, *([node.module] if node.module else [])])
            for alias in node.names:
                local = alias.asname or alias.name
                functions[local] = (base, alias.name)
                modules[local] = f"{base}.{alias.name}" if base else alias.name
    return functions, modules


def _argument(helper: Helper, call: ast.Call, param: str | None) -> ast.expr | None:
    """The argument of ``call`` bound to ``param``."""
    if param is None:
        return None
    if param in helper.keywords:
        for keyword in call.keywords:
            if keyword.arg == param:
                return keyword.value
    if param in helper.positional:
        index = helper.positional.index(param)
        if any(isinstance(arg, ast.Starred) for arg in call.args[: index + 1]):
            return None
        if index < len(call.args):
            return call.args[index]
    return None


def find_helper_calls(
    source: str | ParsedSource, helpers: HelperIndex | None = None, module: str | None = None
) -> list[HelperCall]:
    """Calls passing a string literal to a marked query parameter.

    Helpers defined in ``source`` count, and so do those of ``helpers`` that
    ``source`` imports (``module`` is its name, for relative imports).

    Example:
        >>> src = '''
        ... from typing import Annotated
        ... from sqlty import SQL
        ... def run[T](q: Annotated[str, SQL], t: type[T]) -> T: ...
        ... run("SELECT 1", int)
        ... '''
        >>> [(c.line, c.query) for c in find_helper_calls(src)]
        [(5, 'SELECT 1')]
    """
    parsed = ParsedSource.of(source, module)
    local = parsed.may_define_helpers
    imported = helpers is not None and bool(helpers.modules) and helpers.mentions(parsed.text)
    if not (local or imported):
        return []
    own = index_functions(parsed) if local else {}
    if not own and not imported:
        return []
    functions, modules = (
        _imports(parsed, module if module is not None else parsed.module) if imported else ({}, {})
    )
    registry = helpers.modules if helpers is not None else {}

    def resolve(func: ast.expr) -> Helper | None:
        if isinstance(func, ast.Name):
            if func.id in own:
                return own[func.id]
            if func.id in functions:
                module_name, name = functions[func.id]
                return registry.get(module_name, {}).get(name)
        elif isinstance(func, ast.Attribute) and (prefix := _dotted(func.value)):
            head, _, rest = prefix.partition(".")
            mod = modules.get(head)
            if mod is not None:
                mod = f"{mod}.{rest}" if rest else mod
                return registry.get(mod, {}).get(func.attr)
            return registry.get(prefix, {}).get(func.attr)
        return None

    aliases = parsed.typing_aliases
    calls = []
    for node in parsed.calls:
        if (helper := resolve(node.func)) is None:
            continue
        query = _argument(helper, node, helper.query)
        if not (isinstance(query, ast.Constant) and isinstance(query.value, str)):
            continue
        calls.append(
            HelperCall(node.lineno, query.value, _argument(helper, node, helper.row), aliases)
        )
    return sorted(calls, key=lambda call: call.line)
