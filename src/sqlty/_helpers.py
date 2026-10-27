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
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, NamedTuple

from sqlty._params import ParamArg, arg_type, literal_params
from sqlty._psycopg import UNCHECKED
from sqlty._source import ParsedSource
from sqlty._variants import Unchecked, Variant, query_variants

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable, Sequence

    from sqlty._cli import Settings
    from sqlty._type_extractor import TypeInfo

_TYPE_FORMS = frozenset({"type", "Type", "TypeForm"})
# Names that mark where a helper takes the query's parameters: a ``params`` argument holding
# a sequence or mapping, ``*args`` (asyncpg), or ``**params``.
_PARAMS_ARGUMENT = frozenset({"params", "parameters", "args"})
_PARAMS_VARIADIC = frozenset({"args", "params", "parameters"})
_PARAMS_KEYWORDS = frozenset({"params", "parameters"})


@dataclass(frozen=True)
class Helper:
    """A function with a marked query parameter.

    ``positional`` lists the parameters a call may bind by position, ``keywords``
    those it may bind by name. ``row`` is ``None`` when no ``type[T]`` parameter
    shares ``T`` with the return type. A method is named ``Class.method`` and its
    ``self`` / ``cls`` is not listed; ``kind`` is ``instance`` for a method that is
    only callable on an instance (a call through the class would shift the arguments).
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


class HelperCall(NamedTuple):
    """A call passing a query to a helper's query parameter.

    ``query`` is ``None`` for a psycopg composition with a part that is not a literal, and
    for any other argument that is not a string literal, which is ``dynamic``.
    ``params`` holds the arguments the call passes for the query's placeholders, with the
    type of each where the source states it, or ``None`` when the helper takes no parameters
    or the call does not spell them out. ``driver`` names the driver of a recognised
    ``execute``-like call, and ``single`` the type of a single value it passes where the
    driver takes a sequence or a mapping of parameters.

    With query variants on, ``variants`` holds every query a call built in branches can
    run, ``query`` being the first; ``reason`` says why such a call is not checked.
    """

    line: int
    query: str | None
    row: ast.expr | None
    typing_aliases: dict[str, str]
    params: tuple[ParamArg, ...] | None = None
    end_line: int | None = None
    driver: str | None = None
    single: str | None = None
    dynamic: bool = False
    variants: tuple[Variant, ...] = ()
    reason: str | None = None

    @property
    def typed_lines(self) -> range:
        """The lines of the call when it passes a row type: its strings need no ``SQL[T]``."""
        return (
            range(self.line, (self.end_line or self.line) + 1) if self.row is not None else range(0)
        )


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


def _parameters(args: ast.arguments) -> set[str]:
    return {a.arg for a in [*args.posonlyargs, *args.args, *args.kwonlyargs]}


def _kind(node: ast.FunctionDef | ast.AsyncFunctionDef) -> str:
    """``static``, ``class`` or ``instance`` for a method, by its decorators."""
    names = {_dotted(d) for d in node.decorator_list}
    if "staticmethod" in names:
        return "static"
    return "class" if "classmethod" in names else "instance"


def _rank(node: ast.FunctionDef | ast.AsyncFunctionDef, complete: bool) -> tuple[bool, bool]:
    """Sort key among definitions of one name: complete first, then non-``@overload``."""
    overload = any((_dotted(d) or "").rpartition(".")[2] == "overload" for d in node.decorator_list)
    return not complete, overload


def _definitions(
    parsed: ParsedSource,
) -> list[tuple[str, ast.FunctionDef | ast.AsyncFunctionDef, str]]:
    """Module-level functions and methods as ``(name, node, kind)``; methods are ``Class.name``."""
    return [
        *((node.name, node, "function") for node in parsed.functions),
        *((f"{cls.name}.{node.name}", node, _kind(node)) for cls, node in parsed.methods),
    ]


def _helper(
    name: str,
    node: ast.FunctionDef | ast.AsyncFunctionDef,
    kind: str,
    query: str,
    row: str | None,
) -> Helper:
    args = node.args
    positional = [*args.posonlyargs, *args.args]
    keywords = [*args.args, *args.kwonlyargs]
    names = [a.arg for a in [*positional, *args.kwonlyargs]]
    if kind in ("instance", "class") and positional:
        keywords = [a for a in keywords if a is not positional[0]]
        positional = positional[1:]
    return Helper(
        name=name,
        positional=tuple(a.arg for a in positional),
        keywords=frozenset(a.arg for a in keywords),
        query=query,
        row=row,
        kind=kind,
        params=next((n for n in names if n in _PARAMS_ARGUMENT and n not in (query, row)), None),
        rest=args.vararg.arg if args.vararg and args.vararg.arg in _PARAMS_VARIADIC else None,
        named=args.kwarg.arg if args.kwarg and args.kwarg.arg in _PARAMS_KEYWORDS else None,
    )


def index_functions(source: str | ParsedSource) -> dict[str, Helper]:
    """Functions and methods with a marked query parameter, by name (``Class.method``).

    Example:
        >>> src = "from typing import Annotated\\nfrom sqlty import SQL\\ndef f[T](q: Annotated[str, SQL], t: type[T], /) -> T: ..."
        >>> index_functions(src)["f"]
        Helper(name='f', positional=('q', 't'), keywords=frozenset(), query='q', row='t', kind='function', params=None, rest=None, named=None)
    """
    parsed = ParsedSource.of(source)
    annotated, marker = parsed.marker_names
    if not annotated or not marker:
        return {}
    found: dict[str, tuple[tuple[bool, bool], Helper]] = {}
    for name, node, kind in _definitions(parsed):
        args = node.args
        query = next(
            (
                a.arg
                for a in [*args.posonlyargs, *args.args, *args.kwonlyargs]
                if _is_marked(a.annotation, annotated, marker)
            ),
            None,
        )
        if query is not None:
            row = _row_param(args, node.returns)
            rank = _rank(node, row is not None)
            if name not in found or rank < found[name][0]:
                found[name] = rank, _helper(name, node, kind, query, row)
    return {name: helper for name, (_, helper) in found.items()}


def helper_roots(config_dir: Path | None, checked: Iterable[Path]) -> tuple[Path, ...]:
    """Directories a ``[[tool.sqlty.functions]]`` module is read from, the likeliest first.

    ``config_dir/src``, ``config_dir``, the resolved ``checked`` roots inside it, then
    sqlty's own ``sys.path``.

    Example:
        >>> [root.name for root in helper_roots(Path("/p"), [Path("/p/app"), Path("/q")])[:3]]
        ['src', 'p', 'app']
    """
    project = config_dir.resolve() if config_dir is not None else None
    own = [project / "src", project] if project is not None else []
    inside = [root for root in checked if project is None or root.is_relative_to(project)]
    return tuple(dict.fromkeys([*own, *inside, *(Path(entry) for entry in sys.path if entry)]))


def configured_helpers(
    index: HelperIndex, checked: Sequence[Path]
) -> Callable[[Settings], HelperIndex]:
    """``index`` with the ``[[tool.sqlty.functions]]`` of a file's settings, one fork per project.

    ``checked`` are the checked directories and package roots, passed to `helper_roots`.
    """
    forks: dict[tuple[object, Path | None], HelperIndex] = {}

    def helpers_for(settings: Settings) -> HelperIndex:
        if not settings.functions:
            return index
        key = settings.functions, settings.config_dir
        if (forked := forks.get(key)) is None:
            forks[key] = forked = index.fork()
            roots = helper_roots(settings.config_dir, checked)
            for import_path, query, row in settings.functions:
                forked.register(import_path, query, row, roots=roots)
        return forked

    return helpers_for


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
        self.sources: dict[str, ParsedSource] = {}
        self.packages: set[str] = set()
        self.prefixes: set[str] = set()
        self.namespaces: dict[str, tuple[dict[str, object], dict[str, Any]]] = {}
        self.type_modules: frozenset[str] = frozenset()
        self._pattern: re.Pattern[str] | None = None
        self._names: frozenset[str] | None = None
        self._exports: dict[str, dict[str, tuple[str, str]]] = {}
        self._row_types: dict[tuple[str, frozenset[str]], dict[str, TypeInfo]] = {}
        self._read: dict[tuple[Path, str], ParsedSource | None] = {}

    def fork(self) -> HelperIndex:
        """A copy whose later ``register`` calls do not reach this index; sources are shared."""
        other = HelperIndex()
        other.modules = {module: dict(found) for module, found in self.modules.items()}
        other.sources = self.sources
        other.packages = set(self.packages)
        other.prefixes = set(self.prefixes)
        other.type_modules = self.type_modules
        other._exports = self._exports
        other._row_types = self._row_types
        other._read = self._read
        return other

    def add(self, module: str, source: str | ParsedSource) -> bool:
        """Index the helpers of ``source`` as module ``module``; whether it has any.

        The parsed source is kept, so annotations naming types of ``module`` can be
        resolved without importing it or parsing it again.
        """
        parsed = ParsedSource.of(source)
        if parsed.module is None:
            parsed.module = module
        self.sources[module] = parsed
        if parsed.module is not None and parsed.module.endswith(".__init__"):
            self.packages.add(module)
        parts = module.split(".")
        self.prefixes.update(".".join(parts[:i]) for i in range(1, len(parts) + 1))
        self.namespaces.pop(module, None)
        self._exports.pop(module, None)
        for key in [key for key in self._row_types if key[0] == module]:
            del self._row_types[key]
        if not parsed.may_define_helpers or not (found := index_functions(parsed)):
            return False
        self.modules.setdefault(module, {}).update(found)
        self._pattern = None
        self._names = None
        return True

    def register(
        self,
        import_path: str,
        query: str,
        row: str | None = None,
        *,
        roots: Sequence[Path] = (),
    ) -> None:
        """Register ``import_path`` (``pkg.mod.func`` or ``pkg.mod.Class.method``) as a helper.

        Used for ``[[tool.sqlty.functions]]``: the function needs no marked parameter. Its
        positional parameters are read from its definition in ``pkg.mod``, indexed or else
        read (never imported) from ``pkg/mod.py`` or ``pkg/mod/__init__.py`` under the first
        of ``roots`` holding it; otherwise ``query`` and ``row`` can only be passed by
        keyword. Of ``@overload`` definitions, the first naming both is used unless the
        implementation names them.

        Example:
            >>> index = HelperIndex()
            >>> index.register("db.run", "sql", "form")
            >>> index.modules["db"]["run"].keywords == {"sql", "form"}
            True
        """
        parts = import_path.split(".")
        for i in range(len(parts) - 1, 0, -1):
            module, name = ".".join(parts[:i]), ".".join(parts[i:])
            parsed = self.sources.get(module) or self._module_source(parts[:i], roots)
            definition = min(
                (d for d in (_definitions(parsed) if parsed else []) if d[0] == name),
                key=lambda d: _rank(d[1], {query, row} - {None} <= _parameters(d[1].args)),
                default=None,
            )
            if definition is not None:
                helper = _helper(name, definition[1], definition[2], query, row)
                self.modules.setdefault(module, {})[name] = helper
                break
        else:
            keywords = frozenset(filter(None, [query, row]))
            for i in (len(parts) - 1, len(parts) - 2):
                if i > 0:
                    module, name = ".".join(parts[:i]), ".".join(parts[i:])
                    self.modules.setdefault(module, {})[name] = Helper(
                        name, (), keywords, query, row
                    )
        self._pattern = None
        self._names = None

    def _module_source(self, parts: list[str], roots: Sequence[Path]) -> ParsedSource | None:
        """Module ``parts`` read from the first of ``roots`` holding it; never imported."""
        module = ".".join(parts)
        for root in roots:
            for path, name in (
                (root.joinpath(*parts[:-1], f"{parts[-1]}.py"), module),
                (root.joinpath(*parts, "__init__.py"), f"{module}.__init__"),
            ):
                if (path, name) not in self._read:
                    try:
                        text = path.read_text(encoding="utf-8-sig")
                    except OSError, ValueError:
                        text = None
                    self._read[path, name] = None if text is None else ParsedSource(text, name)
                if (parsed := self._read[path, name]) is not None:
                    return parsed
        return None

    def _reexports(self, module: str, parsed: ParsedSource) -> dict[str, tuple[str, str]]:
        if (exports := self._exports.get(module)) is None:
            exports = self._exports[module] = _imports(parsed, parsed.module or module)[0]
        return exports

    def get(self, module: str, name: str, _depth: int = 0) -> Helper | None:
        """The helper ``name`` of ``module``, following names the module re-exports.

        ``name`` may be ``Class.method``; a package ``__init__`` doing
        ``from .db import fetch_all`` exposes ``fetch_all`` as ``module.fetch_all``.
        """
        if (found := self.modules.get(module, {}).get(name)) is not None:
            return found
        if _depth > 8 or (parsed := self.sources.get(module)) is None:
            return None
        exports = self._reexports(module, parsed)
        head, dot, rest = name.partition(".")
        if (target := exports.get(head)) is None:
            return None
        return self.get(target[0], target[1] + dot + rest, _depth + 1)

    def fragment(self, parsed: ParsedSource, dotted: str) -> tuple[ParsedSource, ast.expr] | None:
        """The value of ``NAME`` or ``module.NAME`` imported by ``parsed`` from a checked module.

        Only a module-level name assigned once counts; re-exports are followed.
        """
        functions, modules = _imports(parsed, parsed.module)
        prefix, _, name = dotted.rpartition(".")
        if not prefix:
            return self._constant(*functions[name]) if name in functions else None
        head, _, rest = prefix.partition(".")
        mod = modules.get(head)
        return self._constant(
            (f"{mod}.{rest}" if rest else mod) if mod is not None else prefix, name
        )

    def row_type(
        self, parsed: ParsedSource, dotted: str, type_modules: frozenset[str]
    ) -> tuple[TypeInfo, dict[str, TypeInfo]] | None:
        """The row type ``NAME`` or ``module.NAME`` that ``parsed`` imports from a checked module.

        Returns it with the row types of its module, which its nested types refer to.
        Re-exports are followed; the module is read from its source, never imported.
        """
        functions, modules = _imports(parsed, parsed.module)
        prefix, _, name = dotted.rpartition(".")
        if not prefix:
            if name not in functions:
                return None
            module, name = functions[name]
        else:
            head, _, rest = prefix.partition(".")
            mod = modules.get(head)
            module = (f"{mod}.{rest}" if rest else mod) if mod is not None else prefix
        return self._row_type(module, name, type_modules)

    def _row_type(
        self, module: str, name: str, type_modules: frozenset[str], _depth: int = 0
    ) -> tuple[TypeInfo, dict[str, TypeInfo]] | None:
        if _depth > 8 or (parsed := self.sources.get(module)) is None:
            return None
        if (defs := self._row_types.get((module, type_modules))) is None:
            from sqlty._type_extractor import extract_type_definitions

            # Modules that import each other's row types see none while either is read.
            self._row_types[module, type_modules] = {}
            defs = extract_type_definitions(parsed, self, module, type_modules)
            self._row_types[module, type_modules] = defs
        if name in defs:
            return defs[name], defs
        exports = self._reexports(module, parsed)
        return self._row_type(*exports[name], type_modules, _depth + 1) if name in exports else None

    def _constant(
        self, module: str, name: str, _depth: int = 0
    ) -> tuple[ParsedSource, ast.expr] | None:
        if _depth > 8 or (parsed := self.sources.get(module)) is None:
            return None
        if (value := parsed.constants.get(name)) is not None:
            return parsed, value
        exports = self._reexports(module, parsed)
        return self._constant(*exports[name], _depth + 1) if name in exports else None

    @property
    def names(self) -> frozenset[str]:
        """Unqualified names of the indexed helpers, methods included."""
        if self._names is None:
            self._names = frozenset(
                name.rpartition(".")[2] for helpers in self.modules.values() for name in helpers
            )
        return self._names

    def mentions(self, source: str) -> bool:
        """Whether ``source`` contains the name of some helper, so it may call one."""
        if self._pattern is None:
            self._pattern = re.compile("|".join(map(re.escape, sorted(self.names))) or r"(?!)")
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


def _call_params(
    helper: Helper, call: ast.Call, parsed: ParsedSource
) -> tuple[ParamArg, ...] | None:
    """The arguments ``call`` passes for the placeholders of its query, or ``None`` when not spelled out.

    A helper takes them as a ``params`` argument (a tuple or list literal, or a dict literal
    of string keys), as ``*args``, or as ``**params``.
    """
    stack = parsed.scopes_of(call)

    def typed(key: int | str, value: ast.expr) -> ParamArg:
        return ParamArg(key, arg_type(value, parsed, stack))

    if helper.params is not None:
        arg = _argument(helper, call, helper.params)
        return literal_params(arg, parsed, stack)
    if helper.rest is not None:
        if any(isinstance(arg, ast.Starred) for arg in call.args):
            return None
        return tuple(typed(i, arg) for i, arg in enumerate(call.args[len(helper.positional) :], 1))
    if helper.named is not None:
        if any(keyword.arg is None for keyword in call.keywords):
            return None
        return tuple(
            typed(keyword.arg, keyword.value)
            for keyword in call.keywords
            if keyword.arg is not None and keyword.arg not in helper.keywords
        )
    return None


def find_helper_calls(
    source: str | ParsedSource,
    helpers: HelperIndex | None = None,
    module: str | None = None,
    *,
    variants: int | None = None,
) -> list[HelperCall]:
    """Calls passing a string literal to a marked query parameter.

    Helpers defined in ``source`` count, and so do those of ``helpers`` that
    ``source`` imports (``module`` is its name, for relative imports). With
    ``variants``, a query built from names bound in branches is read on every path,
    up to that many variants (``query_variants``).

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
    composed = "psycopg" in parsed.text
    if not (local or imported):
        return []
    own = index_functions(parsed) if local else {}
    if not own and not imported:
        return []
    functions, modules = (
        _imports(parsed, module if module is not None else parsed.module) if imported else ({}, {})
    )

    def lookup(module_name: str, name: str) -> Helper | None:
        return helpers.get(module_name, name) if helpers is not None else None

    def module_of(prefix: str) -> str:
        head, _, rest = prefix.partition(".")
        mod = modules.get(head)
        return (f"{mod}.{rest}" if rest else mod) if mod is not None else prefix

    def method_of(cls: str, name: str) -> Helper | None:
        """The method ``name`` of the class spelled ``cls`` in this file."""
        prefix, _, cls_name = cls.rpartition(".")
        if prefix:
            return lookup(module_of(prefix), f"{cls_name}.{name}")
        if (found := own.get(f"{cls}.{name}")) is not None:
            return found
        if cls in functions:
            module_name, original = functions[cls]
            return lookup(module_name, f"{original}.{name}")
        return None

    def resolve(call: ast.Call) -> Helper | None:
        func = call.func
        if isinstance(func, ast.Name):
            if func.id in own:
                return own[func.id]
            if func.id in functions:
                return lookup(*functions[func.id])
        elif isinstance(func, ast.Attribute) and (prefix := _dotted(func.value)):
            if (found := lookup(module_of(prefix), func.attr)) is not None:
                return found
            if (found := method_of(prefix, func.attr)) is not None:
                return found if found.kind != "instance" else None
            if (cls := receiver_class(call, func.value)) is not None:
                return method_of(cls, func.attr)
        return None

    def receiver_class(call: ast.Call, receiver: ast.expr) -> str | None:
        """The class of ``self``, or of a variable or ``self`` attribute with an evident type."""
        stack = parsed.scopes_of(call)
        base = receiver.value if isinstance(receiver, ast.Attribute) else receiver
        if not isinstance(base, ast.Name):
            return None
        for outer, inner in zip(reversed(stack[:-1]), reversed(stack[1:]), strict=True):
            if (
                isinstance(outer, ast.ClassDef)
                and not isinstance(inner, ast.ClassDef)
                and inner.args.args
                and inner.args.args[0].arg == base.id
                and _kind(inner) != "static"
            ):
                if isinstance(receiver, ast.Attribute):
                    return parsed.type_of(stack, receiver.attr, attribute=True)
                return outer.name
        return parsed.type_of(stack, base.id) if base is receiver else None

    def resolve_fragment(source: ParsedSource, dotted: str) -> tuple[ParsedSource, ast.expr] | None:
        if helpers is None:
            return None
        if source.module is None:
            source.module = module
        return helpers.fragment(source, dotted)

    def forwarded(call: ast.Call, query: ast.expr) -> bool:
        """Whether ``query`` is the marked query parameter of a function around ``call``."""
        if not isinstance(query, ast.Name):
            return False
        annotated, marker = parsed.marker_names
        for scope in reversed(parsed.scopes_of(call)):
            if isinstance(scope, ast.ClassDef):
                continue
            args = scope.args
            for arg in [*args.posonlyargs, *args.args, *args.kwonlyargs]:
                if arg.arg == query.id:
                    return _is_marked(arg.annotation, annotated, marker)
        return False

    aliases = parsed.typing_aliases
    calls = []
    scan = parsed.psycopg if composed else None
    pending: list[tuple[ast.Call, Helper, ast.expr]] = []
    names = {name.rpartition(".")[2] for name in own}
    if helpers is not None and imported:
        names |= helpers.names
        names |= {local for local, (_, original) in functions.items() if original in names}
    pattern = re.compile(rf"\b(?:{'|'.join(map(re.escape, sorted(names)))})\b")
    for node in parsed.calls_to(pattern):
        if (helper := resolve(node)) is None:
            continue
        query = _argument(helper, node, helper.query)
        if isinstance(query, ast.Constant) and isinstance(query.value, str):
            calls.append(
                HelperCall(
                    node.lineno,
                    query.value,
                    _argument(helper, node, helper.row),
                    aliases,
                    _call_params(helper, node, parsed),
                    node.end_lineno,
                )
            )
        elif query is not None and not forwarded(node, query):
            pending.append((node, helper, query))
    for node, helper, query in pending:
        text = scan.render(query, resolve_fragment) if scan is not None else None
        call = HelperCall(
            node.lineno,
            None if text == UNCHECKED else text,
            _argument(helper, node, helper.row),
            aliases,
            _call_params(helper, node, parsed),
            node.end_lineno,
            dynamic=text is None,
        )
        if variants is not None and (text is None or text == UNCHECKED):
            found = query_variants(parsed, node, query, variants, resolve_fragment)
            if isinstance(found, Unchecked):
                call = call._replace(
                    reason=found.reason, dynamic=call.dynamic and not found.composed
                )
            else:
                call = call._replace(query=found[0].query, dynamic=False, variants=tuple(found))
        calls.append(call)
    return sorted(calls, key=lambda call: call.line)
