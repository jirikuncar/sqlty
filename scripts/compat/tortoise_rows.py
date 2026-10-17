"""Rows from Tortoise ORM: each field's SQL type per dialect (`SQL_TYPE` and its
`_db_<dialect>` overrides), with the field's Python type (`field_type`)."""

from __future__ import annotations

import inspect
from collections.abc import Iterator

from harness import Row

DIALECTS = {"postgres": "postgres", "mysql": "mysql", "sqlite": "sqlite"}


def _python(field_type: object) -> str | None:
    if not isinstance(field_type, type):
        return "typing.Any"  # JSONField: dict | list
    module = field_type.__module__
    return field_type.__name__ if module == "builtins" else f"{module}.{field_type.__qualname__}"


def rows() -> Iterator[Row]:
    import tortoise
    from tortoise import fields

    version = tortoise.__version__
    for name in sorted(dir(fields)):
        cls = getattr(fields, name)
        if not (inspect.isclass(cls) and issubclass(cls, fields.Field)) or cls is fields.Field:
            continue
        try:
            field = (
                cls(max_length=100)
                if name == "CharField"
                else (cls(max_digits=10, decimal_places=2) if name == "DecimalField" else cls())
            )
        except TypeError:
            continue
        for dialect, db in DIALECTS.items():
            try:
                ddl = field.get_for_dialect(db, "SQL_TYPE")
            except Exception:  # a field this dialect lacks
                continue
            if not ddl or not isinstance(ddl, str):
                continue
            yield Row(
                "tortoise",
                version,
                dialect,
                "column",
                ddl,
                _python(cls.field_type),
                f"fields.{name}",
            )
