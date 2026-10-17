"""Rows from SQLModel: the column type `get_sqlalchemy_type` picks for each annotation,
compiled per dialect, with the annotation as the expected Python type."""

from __future__ import annotations

import datetime
import decimal
import uuid
from collections.abc import Iterator

from harness import Row

ANNOTATIONS = {
    "int": int,
    "float": float,
    "bool": bool,
    "str": str,
    "bytes": bytes,
    "decimal.Decimal": decimal.Decimal,
    "datetime.datetime": datetime.datetime,
    "datetime.date": datetime.date,
    "datetime.time": datetime.time,
    "datetime.timedelta": datetime.timedelta,
    "uuid.UUID": uuid.UUID,
}


def rows() -> Iterator[Row]:
    import sqlmodel
    from sqlalchemy.dialects import mysql, postgresql, sqlite
    from sqlmodel import Field, SQLModel

    dialects = {
        "postgres": postgresql.dialect(),
        "mysql": mysql.dialect(),
        "sqlite": sqlite.dialect(),
    }
    for name, annotation in ANNOTATIONS.items():
        extra = {"max_digits": 10, "decimal_places": 2} if annotation is decimal.Decimal else {}
        model = type(
            f"M_{name.replace('.', '_')}",
            (SQLModel,),
            {
                "__annotations__": {"id": int | None, "c": annotation},
                "id": Field(default=None, primary_key=True),
                "c": Field(**extra),
                "__tablename__": f"t_{name.replace('.', '_')}",
            },
            table=True,
        )
        column_type = model.__table__.c.c.type
        for dialect, engine in dialects.items():
            ddl = column_type.compile(dialect=engine)
            if ddl == "VARCHAR" and dialect == "mysql":
                ddl = "VARCHAR(255)"
            yield Row(
                "sqlmodel", sqlmodel.__version__, dialect, "column", ddl, name, f"annotation {name}"
            )
