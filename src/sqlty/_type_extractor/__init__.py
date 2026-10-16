"""Extract and validate user-defined types against SQL query columns.

Uses Python's runtime typing machinery (``get_type_hints()``, ``issubclass()``)
instead of string comparison for reliable structural validation. This handles
type aliases, ``Optional``, ``from datetime import datetime`` import
variations, forward references, and inheritance correctly.

Two-phase approach:

1. **AST scan** (``ast_fields``): find recognized class definitions (TypedDict,
   NamedTuple, dataclass, BaseModel) and collect the source needed to
   reconstruct them.
2. **Runtime resolve** (``sandbox``): ``exec()`` just the imports, module-level
   type aliases and ``NewType`` assignments (each guarded: a name that fails to
   import or evaluate becomes an unknown placeholder) and the class definition
   in a sandbox namespace, then use ``typing.get_type_hints()`` on each field
   on its own to get actual ``type`` objects. A field that stays unresolved is
   unknown and never reported.

Modules: ``info`` (``TypeInfo``), ``extract`` (``extract_type_definitions``),
``sandbox`` (the trust boundary: the only code that executes user imports),
``compat`` (column/field compatibility), ``subtype`` (subtyping), ``ast_fields``.
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
