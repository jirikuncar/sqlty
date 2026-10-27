"""Find the user-defined row types of a source and resolve their fields, all from the AST."""

from __future__ import annotations

import ast
import types
import typing

from sqlty._source import ParsedSource

if typing.TYPE_CHECKING:
    from sqlty._helpers import HelperIndex

from sqlty._type_extractor.ast_fields import (
    _IMPORTED_TYPES,
    _ast_annotations,
    _ast_key_types,
    _ast_type,
    _base_names,
    _classify_class,
    _extract_fields_from_ast,
    _functional_classes,
    _header_errors,
    _key_errors,
    _lax_fields,
    _lineage,
    _metadata_defaults,
    _own_openness,
    _pydantic_config,
    _set_openness,
    _set_pydantic_names,
)
from sqlty._type_extractor.info import TypeInfo
from sqlty._type_extractor.pydantic_types import pydantic_names
from sqlty._type_extractor.stdlib import stdlib_class
from sqlty._type_extractor.symbols import ModuleTypes


def extract_type_definitions(
    source: str | ParsedSource,
    helpers: HelperIndex | None = None,
    module: str | None = None,
) -> dict[str, TypeInfo]:
    """Extract type definitions from Python source code.

    Finds class definitions that are:
    - ``TypedDict`` subclasses
    - ``NamedTuple`` subclasses
    - ``@dataclass``-decorated classes
    - Pydantic ``BaseModel`` subclasses
    - ``Protocol`` classes, whose attributes a row must have

    Field types are read from the AST; nothing is imported or executed. Names imported
    from modules of ``helpers`` (a ``HelperIndex``) are read from those modules' source,
    standard-library classes from typeshed's class graph (see ``stdlib``) and Pydantic's
    types from a table (see ``pydantic_types``); any other imported name is unknown.
    ``module`` is the name of ``source``, for relative imports.

    Args:
        source: Python source code, or a ``ParsedSource`` whose tree is reused.
        helpers: modules of the checked paths.
        module: dotted name of ``source``.

    Returns:
        Dictionary mapping type name to TypeInfo.
    """
    parsed = ParsedSource.of(source)
    if (tree := parsed.tree) is None:
        return {}

    aliases = parsed.typing_aliases
    imports = parsed.imported_names
    names = ModuleTypes(parsed, helpers, module)

    # Phase 1: AST scan — find recognized classes; TypedDict subclasses
    # are recognized when their base is a TypedDict defined earlier or imported
    recognized: list[tuple[ast.ClassDef, str]] = []  # (node, kind)
    typeddict_nodes: dict[str, ast.ClassDef] = {}
    model_nodes: dict[str, ast.ClassDef] = {}
    dataclass_nodes: dict[str, ast.ClassDef] = {}
    classes = parsed.classes
    if functional := _functional_classes(tree, aliases):
        classes = sorted([*classes, *functional], key=lambda node: node.lineno)
    imported_bases: dict[str, TypeInfo] = {}
    if helpers is not None:
        local = {node.name for node in classes}
        for name in {name for node in classes for name in _base_names(node)} - local:
            if (found := helpers.row_type(parsed, name)) is not None:
                imported_bases[name] = found[0]
    imported_of = {
        kind: {name for name, info in imported_bases.items() if info.kind == kind}
        for kind in ("TypedDict", "BaseModel")
    }
    for node in classes:
        kind = _classify_class(
            node,
            aliases,
            typeddict_nodes.keys() | imported_of["TypedDict"],
            model_nodes.keys() | imported_of["BaseModel"],
        )
        if kind == "TypedDict":
            typeddict_nodes[node.name] = node
        elif kind == "BaseModel":
            model_nodes[node.name] = node
        elif kind == "dataclass":
            dataclass_nodes[node.name] = node
        if kind:
            recognized.append((node, kind))

    if not recognized:
        return {}

    # Phase 2: resolve each class's fields
    result: dict[str, TypeInfo] = {}
    typeddicts = {name: info for name, info in imported_bases.items() if info.kind == "TypedDict"}
    # Dataclasses and Pydantic models, whose fields their subclasses inherit.
    bases = {name: info for name, info in imported_bases.items() if info.kind != "TypedDict"}
    for node, kind in recognized:
        keywords = {k.arg: k.value for k in node.keywords if k.arg}
        total = not (isinstance(t := keywords.get("total"), ast.Constant) and t.value is False)
        own_fields, own_required = _extract_fields_from_ast(
            node,
            aliases,
            total=total,
            defaults_optional=kind in ("dataclass", "BaseModel"),
            kind=kind,
        )
        known = typeddicts if kind == "TypedDict" else bases
        parents = [known[n] for n in _base_names(node) if n in known and known[n].kind == kind]
        inherited = {k: v for p in parents for k, v in p.fields}
        fields = list((inherited | dict(own_fields)).items())
        required = frozenset(
            {k for p in parents for k in p.required} - dict(own_fields).keys() | own_required
        )

        is_typeddict = kind == "TypedDict"
        lineage_of = {
            "TypedDict": typeddict_nodes,
            "BaseModel": model_nodes,
            "dataclass": dataclass_nodes,
        }
        nodes = list(_lineage(node, lineage_of[kind]).values()) if kind in lineage_of else [node]
        defaulted = {name for n in nodes for name in _metadata_defaults(n, aliases)}
        required -= defaulted
        info = TypeInfo(name=node.name, fields=fields, kind=kind, required=required)
        info.lineno = node.lineno
        # A row that has a Protocol's attributes may have more.
        info.allows_extra = kind == "Protocol"
        info.module = module
        if kind == "BaseModel":
            _set_pydantic_names(info, nodes, aliases)
        configs = [
            config for n in nodes if (config := _pydantic_config(n, kind, imports)) is not None
        ]
        if configs:
            config = {key: value for c in configs for key, value in c.items()}
            info.lax = frozenset().union(*(_lax_fields(n, config, aliases, imports) for n in nodes))
        known_types = _IMPORTED_TYPES | pydantic_names(model=bool(configs))
        info.field_types = _ast_key_types(nodes, aliases, names, imports, known_types)
        if configs:
            # Pydantic validates an enum by value: a `str` column fills a `Color` field.
            info.field_types = {
                n: None if _has_enum(tp) else tp for n, tp in info.field_types.items()
            }
        info.annotations = _ast_annotations(nodes, aliases)
        for parent in (p for p in parents if p.module != module):
            info.field_types = parent.field_types | info.field_types
            info.annotations = parent.annotations | info.annotations
            info.lax |= parent.lax
            if kind == "BaseModel":
                info.columns = parent.columns | info.columns
                info.opaque |= parent.opaque
                info.allows_extra |= parent.allows_extra
        info.aliases = aliases
        if is_typeddict:
            info.bases = [p.name for p in parents]
            own = _own_openness(keywords, aliases)
            own_type = (
                _ast_type(own.extra_node, aliases, names, imports, known_types)
                if own and own.extra_node
                else None
            )
            header = _header_errors(info.name, keywords, own, own_type, parents)
            info.errors = list(dict.fromkeys(header + _key_errors(info, info.field_types, parents)))
            _set_openness(info, None if header else own, own_type, parents)
            typeddicts[node.name] = info
        else:
            bases[node.name] = info
        if fields:
            result[node.name] = info

    return result


def _has_enum(tp: typing.Any) -> bool:
    """Whether ``tp`` is an ``enum.Enum`` class, or has one among its arguments."""
    enum = stdlib_class("enum.Enum")
    if isinstance(tp, type) and not isinstance(tp, types.GenericAlias):
        return enum is not None and issubclass(tp, enum)
    return any(map(_has_enum, typing.get_args(tp)))
