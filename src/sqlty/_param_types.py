"""Parameter types: the ``P`` of ``SQL[R, P]`` that ``param-types = true`` writes.

Each ``SQL[R]("--sql\\n...")`` call gets the type of its parameters:

- positional parameters (``%s``, ``$1``, ``?``) are a ``tuple``, ``tuple[()]`` when there are none;
- named parameters (``%(name)s``, ``:name``) are a ``TypedDict`` class, written after the imports
  below a ``# sqlty: generated`` line, as ``json-types`` writes its classes; ``strip`` removes it.

A parameter is ``T | None`` unless it is stored in a ``NOT NULL`` column, and ``typing.Any``
when its type is unknown or cast (``%s::int`` takes anything).
"""

from __future__ import annotations

import ast
import re
import typing

from sqlty._json_types import _Module, _pascal
from sqlty._source import _wrapper_name
from sqlty.core import ParamInfo, analyze_params

if typing.TYPE_CHECKING:
    from sqlty._core import Schema

_MODULES = ("datetime", "decimal", "ipaddress", "typing", "uuid")


def add_param_types(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
) -> str:
    """``source`` with the parameter type of each ``SQL[R]("--sql\\\\n...")`` call written.

    Example:
        >>> source = (
        ...     "from sqlty import SQL\\n"
        ...     "q = SQL[tuple[int]](\\"--sql\\\\nSELECT %(n)s::INTEGER\\")\\n"
        ... )
        >>> print(add_param_types(source))
        import typing
        from sqlty import SQL
        <BLANKLINE>
        <BLANKLINE>
        # sqlty: generated
        class QParams(typing.TypedDict):
            n: typing.Any
        <BLANKLINE>
        <BLANKLINE>
        q = SQL[tuple[int], QParams]("--sql\\nSELECT %(n)s::INTEGER")
        <BLANKLINE>
    """
    wrapper = _wrapper_name(import_style)
    try:
        tree = ast.parse(source)
    except SyntaxError, ValueError:
        return source
    if wrapper is None:
        return source
    module = _Module(tree, source)
    names = {
        id(stmt.value): target.id
        for stmt in ast.walk(tree)
        if isinstance(stmt, ast.Assign | ast.AnnAssign) and stmt.value is not None
        for target in (stmt.targets if isinstance(stmt, ast.Assign) else [stmt.target])
        if isinstance(target, ast.Name)
    }
    edits: list[tuple[int, str]] = []
    for node in ast.walk(tree):
        if not (
            isinstance(node, ast.Call)
            and isinstance(node.func, ast.Subscript)
            and ast.unparse(node.func.value) == wrapper
            and not isinstance(node.func.slice, ast.Tuple)
            and node.args
            and isinstance(node.args[0], ast.Constant)
            and isinstance(node.args[0].value, str)
        ):
            continue
        try:
            params = analyze_params(node.args[0].value, dialect=dialect, schema_sql=schema_sql)
        except RuntimeError:
            continue
        if params is None:
            continue
        types = [(param.name, _param_type(param)) for param in params]
        if not types or any(name is None for name, _ in types):
            text = f"tuple[{', '.join(t for _, t in types) or '()'}]"
        else:
            name = names.get(id(node), "query")
            name = _pascal(name.lower() if name.isupper() else name)
            text = module.named_class([(str(n), t) for n, t in types], name + "Params")
        edits.append((module.span(node.func.slice)[1], f", {text}"))
    if not edits:
        return source
    for at, text in sorted(edits, reverse=True):
        source = source[:at] + text + source[at:]
    return _with_imports(module.with_classes(source))


def _param_type(param: ParamInfo) -> str:
    if param.cast or param.python_type is None:
        return "typing.Any"
    return param.python_type if param.nullable is False else f"{param.python_type} | None"


def _with_imports(source: str) -> str:
    """``source`` with ``import M`` for each module of ``_MODULES`` it names but lacks, in order."""
    tree = ast.parse(source)
    imports = [
        stmt
        for stmt in tree.body
        if isinstance(stmt, ast.Import)
        or (isinstance(stmt, ast.ImportFrom) and stmt.module != "__future__")
    ]
    imported = {
        alias.asname or alias.name
        for stmt in imports
        if isinstance(stmt, ast.Import)
        for alias in stmt.names
    }
    missing = [m for m in _MODULES if m not in imported and re.search(rf"\b{m}\.", source)]
    if not missing or not imports:
        return source
    lines = source.splitlines(keepends=True)
    eol = "\r\n" if "\r\n" in source else "\n"
    for module in reversed(missing):
        before = next(
            (
                stmt
                for stmt in imports
                if isinstance(stmt, ast.ImportFrom) or stmt.names[0].name > module
            ),
            None,
        )
        line = before.lineno - 1 if before else imports[-1].end_lineno or 0
        lines.insert(line, f"import {module}{eol}")
    return "".join(lines)
