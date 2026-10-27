"""Sandboxed runtime resolution of classes: executes only trusted imports and guarded statements."""

from __future__ import annotations

import annotationlib
import ast
import contextlib
import datetime
import functools
import operator
import re
import sys
import textwrap
import typing
import uuid
import warnings

if typing.TYPE_CHECKING:
    from sqlty._helpers import HelperIndex

from sqlty._type_extractor.ast_fields import _TYPE_EXPR_NODES, _get_name, _newtype_assignments
from sqlty._type_extractor.subtype import _union_members, _unwrap_qualifiers


class _Unknown:
    """Stands in for a name that failed to import or evaluate; every operation yields it again."""

    def __or__(self, other: object) -> _Unknown:
        return self

    __ror__ = __or__

    def __getitem__(self, key: object) -> _Unknown:
        return self

    def __call__(self, *args: object, **kwargs: object) -> _Unknown:
        return self

    def __getattr__(self, name: str) -> _Unknown:
        if name.startswith("__"):
            raise AttributeError(name)
        return self


_UNKNOWN_NAME = "__sqlty_unknown__"


def _guard(statement: ast.stmt, names: list[str]) -> str:
    """``statement`` as source that binds ``names`` to the unknown placeholder if it fails."""
    body = textwrap.indent(ast.unparse(statement), "    ")
    fallback = "\n".join(f"    {name} = {_UNKNOWN_NAME}" for name in names)
    return f"try:\n{body}\nexcept Exception:\n{fallback}"


class _Import(typing.NamedTuple):
    """A module-level import. ``module`` is set when it names a module of the checked paths.

    ``name`` is the imported member, or ``None`` when the import binds the module itself.
    """

    local: str
    code: str
    module: str | None = None
    name: str | None = None


_Statement = str | _Import


def _absolute(package: str | None, level: int, name: str | None) -> str | None:
    """Absolute name of ``from <level dots><name> import ...`` inside ``package``.

    Example:
        >>> _absolute("pkg.sub", 1, "ids"), _absolute("pkg.sub", 2, None), _absolute(None, 1, "a")
        ('pkg.sub.ids', 'pkg', 'a')
        >>> _absolute("pkg", 3, "a")
    """
    parts = package.split(".") if package else []
    if level - 1 > len(parts):
        return None
    parts = parts[: len(parts) - (level - 1)]
    return ".".join([*parts, *([name] if name else [])]) or None


_TRUSTED_MODULES = frozenset(
    {"typing", "typing_extensions", "pydantic", "pydantic_core", "annotated_types", "sqlty"}
)


def _trusted(name: str | None, extra: frozenset[str]) -> bool:
    """Whether importing module ``name`` may run: the standard library, `_TRUSTED_MODULES` or ``extra``.

    Example:
        >>> _trusted("os.path", frozenset()), _trusted("app.ids", frozenset()), _trusted("app.ids", frozenset({"app"}))
        (True, False, True)
    """
    if not name:
        return False
    root = name.partition(".")[0]
    return (
        root in sys.stdlib_module_names
        or root in _TRUSTED_MODULES
        or any(name == m or name.startswith(f"{m}.") for m in extra)
    )


def _namespace_statements(
    tree: ast.Module,
    newtypes: set[str],
    index: HelperIndex | None = None,
    module: str | None = None,
    type_modules: frozenset[str] = frozenset(),
) -> list[_Statement]:
    """Guarded source, in file order, of the module-level imports and type aliases.

    A name that cannot be imported or evaluated is bound to the unknown
    placeholder, so one failure does not hide the other names. An import of a
    module in ``index`` is an `_Import` naming it, to be read from its source
    instead. Any other import is executed only if it names the standard
    library, a trusted module or one of ``type_modules``, else it binds the
    placeholder. ``module`` is the dotted name of ``tree`` (``pkg.__init__``
    for a package), for relative imports.

    Example:
        >>> _namespace_statements(ast.parse("import os.path\\nimport a.b\\nX = int"), set())[0].code.splitlines()[:2]
        ['try:', '    import os.path']
    """
    package = None
    if module is not None:
        package = (
            module.removesuffix(".__init__")
            if module.endswith(".__init__")
            else module.rpartition(".")[0]
        )
    statements: list[_Statement] = []
    for node in tree.body:
        if isinstance(node, ast.Import):
            for alias in node.names:
                bound = alias.asname or alias.name.split(".")[0]
                target = alias.name if alias.asname else bound
                checked = index is not None and target in index.prefixes
                code = (
                    _guard(ast.Import(names=[alias]), [bound])
                    if checked or _trusted(alias.name, type_modules)
                    else f"{bound} = {_UNKNOWN_NAME}"
                )
                statements.append(_Import(bound, code, target if checked else None))
        elif isinstance(node, ast.ImportFrom) and node.module != "__future__":
            base = _absolute(package, node.level, node.module) if node.level else node.module
            for alias in node.names:
                if alias.name != "*":
                    from_ = ast.ImportFrom(node.module, [alias], node.level)
                    local = alias.asname or alias.name
                    checked = (
                        index is not None
                        and base is not None
                        and (base in index.prefixes or f"{base}.{alias.name}" in index.prefixes)
                    )
                    code = (
                        _guard(from_, [local])
                        if checked or (not node.level and _trusted(node.module, type_modules))
                        else f"{local} = {_UNKNOWN_NAME}"
                    )
                    statements.append(_Import(local, code, base if checked else None, alias.name))
        elif isinstance(node, ast.TypeAlias) and isinstance(node.name, ast.Name):
            statements.append(_guard(node, [node.name.id]))
        elif (
            isinstance(node, ast.Assign)
            and len(node.targets) == 1
            and isinstance(node.targets[0], ast.Name)
            and (
                node.targets[0].id in newtypes
                or all(isinstance(n, _TYPE_EXPR_NODES) for n in ast.walk(node.value))
            )
        ):
            statements.append(_guard(node, [node.targets[0].id]))
        elif (
            isinstance(node, ast.AnnAssign)
            and isinstance(node.target, ast.Name)
            and node.value is not None
            and _get_name(node.annotation) == "TypeAlias"
            and all(isinstance(n, _TYPE_EXPR_NODES) for n in ast.walk(node.value))
        ):
            statements.append(_guard(node, [node.target.id]))
    return statements


class _ModuleProxy:
    """A module of the checked paths, whose attributes are read from its source."""

    def __init__(self, index: HelperIndex, name: str) -> None:
        self._index = index
        self._name = name

    def __getattr__(self, attr: str) -> object:
        if attr.startswith("__"):
            raise AttributeError(attr)
        return _lookup(self._index, self._name, attr)


def _lookup(index: HelperIndex, module: str, name: str) -> object:
    """``module.name`` from the source of ``module``, the unknown placeholder if not found."""
    if module in index.sources:
        namespace, pending = _module_namespace(index, module)
        if name in namespace:
            return namespace[name]
        if (imported := pending.pop(name, None)) is not None:
            namespace[name] = value = _import_value(index, imported)
            return value
    if f"{module}.{name}" in index.prefixes:
        return _ModuleProxy(index, f"{module}.{name}")
    return _Unknown()


def _import_value(index: HelperIndex, imported: _Import) -> object:
    assert imported.module is not None
    if imported.name is None:
        return _ModuleProxy(index, imported.module)
    return _lookup(index, imported.module, imported.name)


def _identifiers(statements: typing.Iterable[str]) -> set[str]:
    return set(re.findall(r"\w+", "\n".join(statements)))


def _populate(
    index: HelperIndex | None,
    namespace: dict[str, object],
    statements: list[_Statement],
    wanted: set[str],
    skip: bool,
) -> dict[str, _Import]:
    """Run ``statements`` in order in ``namespace``.

    Imports of checked modules are bound from source when their name is in
    ``wanted``. The others are dropped if ``skip`` is set, else returned to be
    bound on demand.
    """
    pending: dict[str, _Import] = {}
    code: list[str] = []

    def flush() -> None:
        if code:
            compiled = compile("\n".join(code), "<sqlty-type-check>", "exec", dont_inherit=True)
            exec(compiled, namespace)  # noqa: S102
            code.clear()

    for statement in statements:
        if isinstance(statement, str):
            code.append(statement)
        elif index is None or statement.module is None:
            code.append(statement.code)
        elif statement.local in wanted:
            flush()
            namespace[statement.local] = _import_value(index, statement)
        elif not skip:
            pending[statement.local] = statement
    flush()
    return pending


def _module_namespace(
    index: HelperIndex, module: str
) -> tuple[dict[str, object], dict[str, _Import]]:
    """Names of the type-defining statements of ``module``, read from its source once.

    Imports of other checked modules are bound on first use. The result is
    cached in ``index``; a module being built (an import cycle) is seen as
    far as it got.
    """
    if module in index.namespaces:
        return index.namespaces[module]
    # ``__name__`` makes a NewType defined there name ``module``, not ``__main__``.
    namespace: dict[str, object] = {
        "__builtins__": __builtins__,
        "__name__": module,
        _UNKNOWN_NAME: _Unknown(),
    }
    pending: dict[str, _Import] = {}
    index.namespaces[module] = namespace, pending
    if (tree := index.sources[module].tree) is None:
        return namespace, pending
    newtypes = _newtype_assignments(tree, index.sources[module].typing_aliases)
    name = f"{module}.__init__" if module in index.packages else module
    statements = _namespace_statements(tree, set(newtypes), index, name, index.type_modules)
    wanted = _identifiers(s for s in statements if isinstance(s, str))
    with contextlib.suppress(Exception):
        pending |= _populate(index, namespace, statements, wanted, skip=False)
    return namespace, pending


def _resolve_class(
    nodes: list[ast.ClassDef],
    import_stmts: list[_Statement],
    full_source: str,
    index: HelperIndex | None = None,
) -> tuple[type, dict[str, object]] | None:
    """Try to build the last of ``nodes`` at runtime, returning it with its namespace.

    Executes just the necessary imports (of the standard library and trusted
    modules only, see `_namespace_statements`) and aliases + the class definitions
    (bases first) in a sandboxed namespace. Returns None if the classes
    cannot be built. Names imported from modules of ``index`` are read from
    their source, and only those the classes use.
    """
    class_sources = [
        segment for node in nodes if (segment := ast.get_source_segment(full_source, node))
    ]
    if len(class_sources) != len(nodes):
        return None

    # Build a safe namespace with common types pre-loaded
    namespace: dict[str, object] = {
        "__builtins__": __builtins__,
        "datetime": datetime,
        "uuid": uuid,
        _UNKNOWN_NAME: _Unknown(),
    }
    wanted = _identifiers([*class_sources, *(s for s in import_stmts if isinstance(s, str))])

    # The checked code's deprecation warnings are not sqlty's to print.
    try:
        with warnings.catch_warnings(action="ignore"):
            _populate(index, namespace, import_stmts, wanted, skip=True)
            # dont_inherit=True prevents this module's `from __future__ import
            # annotations` from turning the exec'd class annotations into
            # ForwardRef strings — we need real type objects for get_type_hints.
            code = compile(
                "\n".join(class_sources), "<sqlty-type-check>", "exec", dont_inherit=True
            )
            exec(code, namespace)  # noqa: S102
    except Exception:
        return None

    cls = namespace.get(nodes[-1].name)
    return (cls, namespace) if isinstance(cls, type) else None


def _has_unknown(tp: typing.Any) -> bool:
    """Whether ``tp`` is, or contains, an unresolved name or forward reference."""
    if isinstance(tp, (_Unknown, annotationlib.ForwardRef)):
        return True
    if isinstance(tp, (list, tuple)):
        return any(map(_has_unknown, tp))
    return typing.get_origin(tp) is not typing.Literal and any(
        map(_has_unknown, typing.get_args(tp))
    )


def _resolve_alias(tp: typing.Any) -> typing.Any:
    """Look through ``type X = ...`` aliases, also within unions."""
    while isinstance(tp, typing.TypeAliasType):
        tp = tp.__value__
    if len(members := _union_members(tp)) > 1:
        return functools.reduce(operator.or_, map(_resolve_alias, members))
    return tp


def _type_hints(cls: type, namespace: dict[str, object]) -> dict[str, typing.Any]:
    """Resolve each field's type on its own, with qualifiers (``ReadOnly`` etc.) dropped.

    Fields that cannot be resolved are left out.
    """
    hints: dict[str, typing.Any] = {}
    for name, raw in annotationlib.get_annotations(
        cls, format=annotationlib.Format.FORWARDREF
    ).items():
        holder = type("_Field", (), {"__annotations__": {name: raw}})
        try:
            tp = _resolve_alias(
                typing.get_type_hints(holder, globalns=namespace, localns=namespace)[name]
            )
        except Exception:
            continue
        if not _has_unknown(tp):
            hints[name] = tp
    return hints


def _own_extra_type(cls: type, namespace: dict[str, object]) -> typing.Any:
    """The ``extra_items`` type ``cls`` declares, forward references resolved, or None."""
    tp = _unwrap_qualifiers(cls.__dict__.get("__extra_items__"))
    if isinstance(tp, str):
        tp = annotationlib.ForwardRef(tp)
    if isinstance(tp, annotationlib.ForwardRef):
        try:
            tp = typing.evaluate_forward_ref(tp, globals=namespace, locals=namespace)
        except Exception:
            return None
    return None if _has_unknown(tp) else type(None) if tp is None else tp
