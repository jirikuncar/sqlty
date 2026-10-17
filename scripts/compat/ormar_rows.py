"""Rows from ormar: each field's SQLAlchemy column type compiled per dialect, with the
field's Python type (`__type__`)."""

from __future__ import annotations

from collections.abc import Iterator

from harness import Row

FIELDS = {
    "Integer": {},
    "BigInteger": {},
    "SmallInteger": {},
    "String": {"max_length": 100},
    "Text": {},
    "Float": {},
    "Decimal": {"max_digits": 10, "decimal_places": 2},
    "Boolean": {},
    "Date": {},
    "DateTime": {},
    "Time": {},
    "JSON": {},
    "UUID": {},
    "LargeBinary": {"max_length": 100},
}


def rows() -> Iterator[Row]:
    import warnings

    import ormar
    from sqlalchemy.dialects import mysql, postgresql, sqlite

    warnings.simplefilter("ignore")
    dialects = {
        "postgres": postgresql.dialect(),
        "mysql": mysql.dialect(),
        "sqlite": sqlite.dialect(),
    }
    for name, kwargs in FIELDS.items():
        field = getattr(ormar, name)(**kwargs)
        python_type = field.__type__
        module = python_type.__module__
        python = (
            "typing.Any"
            if name == "JSON"
            else python_type.__name__
            if module == "builtins"
            else f"{module}.{python_type.__qualname__}"
        )
        for dialect, engine in dialects.items():
            ddl = field.column_type.compile(dialect=engine)
            yield Row("ormar", ormar.__version__, dialect, "column", ddl, python, f"ormar.{name}")
