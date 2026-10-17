"""Rows from Django: the DDL each model field gets per backend, and the Python type the
field's value has once Django's converters ran.

Column axis: `DatabaseWrapper.data_types` (`_data_types` on MySQL) per backend, formatted
with `max_length=100`, `max_digits=10`, `decimal_places=2`. Expression axis: the
aggregates and arithmetic of the issue, with the `output_field` Django gives them. The
expected types are Django's, above the driver: a mismatch is a converter difference to
document unless sqlty's type is not the driver's.
"""

from __future__ import annotations

from collections.abc import Iterator

from harness import Row

# The Python type of a field's value, after Django's converters (Django documentation,
# "Model field reference").
PYTHON = {
    "AutoField": "int",
    "BigAutoField": "int",
    "SmallAutoField": "int",
    "BinaryField": "bytes",
    "BooleanField": "bool",
    "CharField": "str",
    "DateField": "datetime.date",
    "DateTimeField": "datetime.datetime",
    "DecimalField": "decimal.Decimal",
    "DurationField": "datetime.timedelta",
    "FileField": "str",
    "FilePathField": "str",
    "FloatField": "float",
    "IntegerField": "int",
    "BigIntegerField": "int",
    "IPAddressField": "str",
    "GenericIPAddressField": "str",
    "JSONField": "typing.Any",
    "PositiveBigIntegerField": "int",
    "PositiveIntegerField": "int",
    "PositiveSmallIntegerField": "int",
    "SlugField": "str",
    "SmallIntegerField": "int",
    "TextField": "str",
    "TimeField": "datetime.time",
    "UUIDField": "uuid.UUID",
}
PARAMS = {"max_length": 100, "max_digits": 10, "decimal_places": 2}
# (expression, Django output field) over `t (i integer, d numeric(10, 2), ts <datetime>)`.
EXPRESSIONS = [
    ("count(i)", "IntegerField"),
    ("sum(i)", "IntegerField"),
    ("avg(i)", "FloatField"),
    ("avg(d)", "DecimalField"),
    ("max(d)", "DecimalField"),
    ("i / i", "IntegerField"),
    ("i * d", "DecimalField"),
    ("ts - ts", "DurationField"),
]


def rows() -> Iterator[Row]:
    import django
    from django.conf import settings

    if not settings.configured:
        settings.configure(
            DATABASES={"default": {"ENGINE": "django.db.backends.sqlite3", "NAME": ":memory:"}},
            INSTALLED_APPS=[],
        )
        django.setup()
    from django.db.backends.mysql.base import DatabaseWrapper as MySQL
    from django.db.backends.postgresql.base import DatabaseWrapper as Postgres
    from django.db.backends.sqlite3.base import DatabaseWrapper as Sqlite

    version = django.__version__
    backends = {
        "postgres": Postgres.data_types,
        "mysql": MySQL._data_types,
        "sqlite": Sqlite.data_types,
    }
    for dialect, data_types in backends.items():
        for field, ddl in data_types.items():
            ddl = ddl(PARAMS) if callable(ddl) else ddl % PARAMS
            yield Row("django", version, dialect, "column", ddl, PYTHON.get(field), field)
        schema = (
            f"CREATE TABLE t (i {data_types['IntegerField']} NOT NULL, "
            f"d {data_types['DecimalField'] % PARAMS if isinstance(data_types['DecimalField'], str) else data_types['DecimalField'](PARAMS)} NOT NULL, "
            f"ts {data_types['DateTimeField']} NOT NULL);"
        )
        for sql, field in EXPRESSIONS:
            yield Row(
                "django",
                version,
                dialect,
                "expression",
                sql,
                PYTHON[field],
                f"output_field {field}",
                schema,
            )
