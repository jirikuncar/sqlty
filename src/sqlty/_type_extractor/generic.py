"""Generic row types with their type arguments applied: ``SQL[Row[int]]`` for ``class Row[T]``."""

from __future__ import annotations

import ast
import copy
import re

from sqlty._type_extractor.ast_fields import _ast_type
from sqlty._type_extractor.info import TypeInfo
from sqlty._type_extractor.symbols import ModuleTypes


def specialize(
    info: TypeInfo, node: ast.ClassDef, args: list[ast.expr], names: ModuleTypes
) -> TypeInfo | None:
    """``info`` with the type parameters of its class ``node`` replaced by ``args``.

    The parameters are those of ``class Row[T]`` or of a ``Generic[T]`` base. ``None`` when
    the class takes a different number of arguments. An argument sqlty cannot resolve leaves
    the fields it fills unknown, as an unresolved annotation does. ``names`` are those of the
    module that names ``Row[...]``.

    Example:
        >>> from sqlty._source import ParsedSource
        >>> from sqlty._type_extractor import extract_type_definitions
        >>> parsed = ParsedSource("import datetime\\nfrom typing import TypedDict\\nclass Row[T](TypedDict):\\n    ids: list[T]")
        >>> row = extract_type_definitions(parsed)["Row"]
        >>> arg = ast.parse("datetime.date", mode="eval").body
        >>> specialize(row, parsed.classes[0], [arg], ModuleTypes(parsed, None)).field_types
        {'ids': list[datetime.date]}
    """
    params = _type_params(node)
    if not params or len(params) != len(args):
        return None
    texts = dict(zip(params, (ast.unparse(arg) for arg in args), strict=True))
    nodes = dict(zip(params, args, strict=True))
    words = re.compile(r"\b(" + "|".join(map(re.escape, params)) + r")\b")

    def text(annotation: str) -> str:
        return words.sub(lambda match: texts[match[0]], annotation)

    aliases, imports = names.parsed.typing_aliases, names.parsed.imported_names
    out = copy.copy(info)
    out.fields = [(name, text(annotation)) for name, annotation in info.fields]
    out.annotations = {
        name: _Substitute(nodes).visit(copy.deepcopy(annotation))
        for name, annotation in info.annotations.items()
    }
    out.field_types = {
        name: _ast_type(out.annotations[name], aliases, names, imports)
        if name in out.annotations
        else tp
        for name, tp in info.field_types.items()
    }
    return out


def _type_params(node: ast.ClassDef) -> list[str]:
    """``T`` of ``class Row[T]``, or of a ``Generic[T]`` base."""
    if node.type_params:
        return [param.name for param in node.type_params if isinstance(param, ast.TypeVar)]
    for base in node.bases:
        if isinstance(base, ast.Subscript) and ast.unparse(base.value).endswith("Generic"):
            items = base.slice.elts if isinstance(base.slice, ast.Tuple) else [base.slice]
            return [item.id for item in items if isinstance(item, ast.Name)]
    return []


class _Substitute(ast.NodeTransformer):
    def __init__(self, nodes: dict[str, ast.expr]) -> None:
        self.nodes = nodes

    def visit_Name(self, node: ast.Name) -> ast.expr:
        return copy.deepcopy(self.nodes[node.id]) if node.id in self.nodes else node
