"""Standard-library classes, as nominal classes built from typeshed's class graph."""

from __future__ import annotations

import functools
import sys

from sqlty._type_extractor._stdlib import CLASSES, ENUMS, MODULES
from sqlty._type_extractor.info import PYTHON_TYPE_MAP


@functools.cache
def stdlib_class(qualname: str) -> type | None:
    """The class ``module.Name`` of the standard library, or ``None`` if it is unknown.

    A column type is itself; any other class is a stand-in deriving from the column
    types the real one does, so it fits the same columns, and from the stand-in of
    ``enum.Enum`` when it is an enum. Nothing is imported.

    Example:
        >>> stdlib_class("enum.StrEnum").__mro__[1], stdlib_class("pathlib.Path").__mro__[1]
        (<class 'str'>, <class 'object'>)
        >>> stdlib_class("ipaddress.IPv4Address") is __import__("ipaddress").IPv4Address
        True
        >>> issubclass(stdlib_class("http.HTTPStatus"), stdlib_class("enum.Enum"))
        True
    """
    if "." in qualname and isinstance(column := PYTHON_TYPE_MAP.get(qualname), type):
        return column
    module, _, name = qualname.rpartition(".")
    if (bases := CLASSES.get(qualname)) is None or not _available(module):
        return None
    found = tuple(PYTHON_TYPE_MAP[base] for base in bases)
    if qualname in ENUMS and (enum := stdlib_class("enum.Enum")) is not None:
        found += (enum,)
    return type(name, found, {"__module__": module})


def _available(module: str) -> bool:
    parts = module.split(".")
    return all(
        low <= sys.version_info[:2] <= (high or sys.version_info[:2])
        for low, high in (
            MODULES.get(".".join(parts[:i])) or ((0, 0), None) for i in range(1, len(parts) + 1)
        )
    )
