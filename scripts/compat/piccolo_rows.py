"""Rows from Piccolo: each column type's SQL per engine (`column_type`), with the
column's Python type (`value_type`)."""

from __future__ import annotations

import inspect
from collections.abc import Iterator
from unittest import mock

from harness import Row

ENGINES = {"postgres": "postgres", "sqlite": "sqlite"}


def _python(value_type: object) -> str | None:
    if not isinstance(value_type, type):
        return None
    module = value_type.__module__
    return value_type.__name__ if module == "builtins" else f"{module}.{value_type.__qualname__}"


def rows() -> Iterator[Row]:
    import warnings

    import piccolo
    from piccolo.columns import column_types
    from piccolo.columns.base import Column

    warnings.simplefilter("ignore")
    version = piccolo.__VERSION__ if hasattr(piccolo, "__VERSION__") else "1.36.0"
    for name in sorted(dir(column_types)):
        cls = getattr(column_types, name)
        if (
            not (inspect.isclass(cls) and issubclass(cls, Column))
            or cls is Column
            or name == "PrimaryKey"
        ):
            continue
        try:
            column = cls()
        except TypeError:  # needs arguments: Array, ForeignKey
            continue
        for dialect, engine in ENGINES.items():
            meta = type(column._meta)
            with mock.patch.object(
                meta, "engine_type", new_callable=mock.PropertyMock, return_value=engine
            ):
                ddl = column.column_type
            # JSON values are decoded by Piccolo: any JSON value.
            python = (
                "typing.Any"
                if name in ("JSON", "JSONB")
                else _python(getattr(column, "value_type", None))
            )
            yield Row("piccolo", version, dialect, "column", ddl, python, f"columns.{name}")
