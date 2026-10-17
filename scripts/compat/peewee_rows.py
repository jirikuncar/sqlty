"""Rows from peewee: each field's column type per database (`field_type`, through the
database's `field_types`), with the field's Python value type."""

from __future__ import annotations

from collections.abc import Iterator

from harness import Row

# The Python type of each field's value (peewee documentation, "Field types table").
PYTHON = {
    "AutoField": "int",
    "BigAutoField": "int",
    "IntegerField": "int",
    "BigIntegerField": "int",
    "SmallIntegerField": "int",
    "FloatField": "float",
    "DoubleField": "float",
    "DecimalField": "decimal.Decimal",
    "CharField": "str",
    "FixedCharField": "str",
    "TextField": "str",
    "BlobField": "bytes",
    "UUIDField": "uuid.UUID",
    "BinaryUUIDField": "uuid.UUID",
    "DateTimeField": "datetime.datetime",
    "DateField": "datetime.date",
    "TimeField": "datetime.time",
    "TimestampField": "datetime.datetime",
    "BooleanField": "bool",
    "IPField": "str",
    "BitField": "int",
}
SIZES = {
    "VARCHAR": "VARCHAR(255)",
    "CHAR": "CHAR(10)",
    "DECIMAL": "DECIMAL(10, 5)",
    "NUMERIC": "NUMERIC(10, 5)",
}


def rows() -> Iterator[Row]:
    import peewee

    databases = {
        "postgres": peewee.PostgresqlDatabase,
        "mysql": peewee.MySQLDatabase,
        "sqlite": peewee.SqliteDatabase,
    }
    for name, python in PYTHON.items():
        field = getattr(peewee, name, None)
        if field is None:
            continue
        for dialect, database in databases.items():
            ddl = database.field_types.get(field.field_type, field.field_type)
            ddl = SIZES.get(ddl, ddl)
            yield Row(
                "peewee", peewee.__version__, dialect, "column", ddl, python, f"peewee.{name}"
            )
