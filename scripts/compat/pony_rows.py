"""Rows from Pony ORM: the column type its schema generator writes for each Python
attribute type, per provider, with that Python type as the expected one.

Pony's PostgreSQL and MySQL providers connect when they bind, so those run only with
`SQLTY_PG_DSN` or `SQLTY_MYSQL_DSN`; SQLite always runs.
"""

from __future__ import annotations

import datetime
import decimal
import os
import re
import uuid
from collections.abc import Iterator
from urllib.parse import parse_qs, urlparse

from harness import Row

ATTRIBUTES = {
    "b": (bool, "bool"),
    "s": (str, "str"),
    "i": (int, "int"),
    "f": (float, "float"),
    "d": (decimal.Decimal, "decimal.Decimal"),
    "dt": (datetime.datetime, "datetime.datetime"),
    "da": (datetime.date, "datetime.date"),
    "t": (datetime.time, "datetime.time"),
    "td": (datetime.timedelta, "datetime.timedelta"),
    "u": (uuid.UUID, "uuid.UUID"),
    "by": (bytes, "bytes"),
    "j": (None, "typing.Any"),
}


def rows() -> Iterator[Row]:
    import pony
    from pony.orm import Database, Json, PrimaryKey, Required

    for dialect, provider, kwargs in _providers():
        db = Database()
        attributes = {name: Required(py_type or Json) for name, (py_type, _) in ATTRIBUTES.items()}
        type("T", (db.Entity,), {"id": PrimaryKey(int, auto=True), **attributes})
        db.bind(provider=provider, **kwargs)
        db.generate_mapping(create_tables=False, check_tables=False)
        script = db.schema.generate_create_script()
        for name, (_, python) in ATTRIBUTES.items():
            match = re.search(rf'["`]{name}["`] (.+?) NOT NULL', script)
            if match:
                yield Row(
                    "pony",
                    pony.__version__,
                    dialect,
                    "column",
                    match.group(1),
                    python,
                    f"attribute {python}",
                )


def _providers() -> Iterator[tuple[str, str, dict[str, object]]]:
    yield "sqlite", "sqlite", {"filename": ":memory:"}
    if dsn := os.environ.get("SQLTY_PG_DSN"):
        url = urlparse(dsn)
        query = {key: values[0] for key, values in parse_qs(url.query).items()}
        yield (
            "postgres",
            "postgres",
            {
                "user": url.username or "postgres",
                "host": query.get("host", url.hostname or "localhost"),
                "port": int(query.get("port", url.port or 5432)),
                "database": url.path.lstrip("/") or "postgres",
                **({"password": url.password} if url.password else {}),
            },
        )
    if dsn := os.environ.get("SQLTY_MYSQL_DSN"):
        url = urlparse(dsn)
        query = {key: values[0] for key, values in parse_qs(url.query).items()}
        yield (
            "mysql",
            "mysql",
            {
                "user": url.username or "root",
                "db": url.path.lstrip("/"),
                **(
                    {"unix_socket": query["unix_socket"]}
                    if "unix_socket" in query
                    else {"host": url.hostname}
                ),
            },
        )
