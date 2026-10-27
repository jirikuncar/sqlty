"""Extract and validate user-defined types against SQL query columns.

Row types are read from source only: nothing of the checked code is imported, executed
or evaluated. ``ast_fields`` finds the recognized classes (TypedDict, NamedTuple,
dataclass, BaseModel, Protocol) and resolves each field's annotation to a ``typing``
object (``int``, ``list[str]``, ``UserId | None``), going through:

- the module's own names, ``symbols.ModuleTypes``: aliases, ``NewType``s and classes;
- the modules of the checked paths, read from their source (``HelperIndex``);
- the standard library, as nominal classes built from typeshed's class graph (``stdlib``);
- Pydantic's types, from a table of what each takes (``pydantic_types``).

Any other name is unknown, and a field of unknown type is never reported.

Modules: ``info`` (``TypeInfo``), ``extract`` (``extract_type_definitions``),
``compat`` (column/field compatibility), ``subtype`` (subtyping), ``ast_fields``,
``symbols``, ``stdlib``, ``pydantic_types`` and ``generic`` (``SQL[Row[int]]``).
"""

from sqlty._type_extractor.compat import (
    admits_none,
    check_type_compatibility,
    column_literal_values,
    declared_literal_values,
    field_accepts,
    field_type,
    is_optional,
    parse_tuple_annotation,
)
from sqlty._type_extractor.extract import extract_type_definitions
from sqlty._type_extractor.generic import specialize
from sqlty._type_extractor.info import TypeInfo

__all__ = [
    "TypeInfo",
    "admits_none",
    "check_type_compatibility",
    "column_literal_values",
    "declared_literal_values",
    "extract_type_definitions",
    "field_accepts",
    "field_type",
    "is_optional",
    "parse_tuple_annotation",
    "specialize",
]
