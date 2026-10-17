"""Rows from SQLObject: each column class's SQL per database (`_postgresType`,
`_mysqlType`, `_sqliteType`), with the column's Python value type."""

from __future__ import annotations

from collections.abc import Iterator

from harness import Row

PYTHON = {
    "BLOBCol": "bytes",
    "BigIntCol": "int",
    "BoolCol": "bool",
    "CurrencyCol": "decimal.Decimal",
    "DateCol": "datetime.date",
    "DateTimeCol": "datetime.datetime",
    "DecimalCol": "decimal.Decimal",
    "DecimalStringCol": "decimal.Decimal",
    "FloatCol": "float",
    "IntCol": "int",
    "JSONCol": "typing.Any",
    "JsonbCol": "typing.Any",
    "MediumIntCol": "int",
    "SmallIntCol": "int",
    "StringCol": "str",
    "TimeCol": "datetime.time",
    "TimedeltaCol": "datetime.timedelta",
    "TimestampCol": "datetime.datetime",
    "TinyIntCol": "int",
    "UnicodeCol": "str",
    "UuidCol": "uuid.UUID",
}
KWARGS = {
    "DecimalCol": {"size": 10, "precision": 2},
    "CurrencyCol": {},
    "DecimalStringCol": {"size": 10, "precision": 2},
}


def rows() -> Iterator[Row]:
    from types import SimpleNamespace

    import sqlobject
    from sqlobject import col, styles

    so_class = SimpleNamespace(
        sqlmeta=SimpleNamespace(style=styles.defaultStyle, table="t"), __name__="T"
    )

    version = getattr(sqlobject, "version", "3.14.0")
    for name, python in PYTHON.items():
        cls = getattr(col, f"SO{name}", None)
        if cls is None:
            continue
        try:
            column = cls(name="c", soClass=so_class, creationOrder=0, **KWARGS.get(name, {}))
        except TypeError, AttributeError, AssertionError:
            continue
        for dialect, method in (
            ("postgres", "_postgresType"),
            ("mysql", "_mysqlType"),
            ("sqlite", "_sqliteType"),
        ):
            try:
                ddl = getattr(column, method)()
            except Exception:  # a column a database lacks
                continue
            if isinstance(ddl, str) and ddl:
                yield Row("sqlobject", str(version), dialect, "column", ddl, python, f"col.{name}")
