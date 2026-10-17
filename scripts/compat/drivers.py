"""The oracle: what each driver returns for each column type, read from live servers.

For every type of `tests/fixtures/driver_values.toml`, plus the types the drivers are
known to disagree on, a value is stored in a one-column table and read back through each
driver; `type(value)` is recorded. The reference driver of each dialect (see
`docs/reference/types.md`) is the expected type of a harness row.

    SQLTY_PG_DSN=postgresql://postgres@localhost/postgres \\
    SQLTY_MYSQL_DSN='mysql://root@localhost/probe?unix_socket=/tmp/mysql.sock' \\
    uv run --with psycopg2-binary --with asyncpg --with pymysql --with mysqlclient \\
        --with aiosqlite --with psycopg python scripts/compat/run.py drivers
"""

from __future__ import annotations

import asyncio
import os
import sqlite3
import tomllib
from collections.abc import Callable, Iterator
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse

from harness import Row

FIXTURE = Path(__file__).parent.parent.parent / "tests" / "fixtures" / "driver_values.toml"
REFERENCE = {"postgres": "psycopg", "mysql": "pymysql", "sqlite": "sqlite3"}

# Types the drivers disagree on, beyond the fixture.
EXTRA = {
    "postgres": {
        '"char"': "'a'",
        "INT4RANGE": "'[1,5)'",
        "TSTZRANGE": "'[2024-01-01, 2024-02-01)'",
        "INT4MULTIRANGE": "'{[1,2)}'",
        "POINT": "'(1,2)'",
        "DATE": "'infinity'",
        "TIMESTAMP": "'infinity'",
    },
    "mysql": {"DATE": "'0000-00-00'", "TIME": "'839:00:00'"},
    "sqlite": {},
}

Fetch = Callable[[str, str], Any]


def rows() -> Iterator[Row]:
    cases = tomllib.loads(FIXTURE.read_text())
    for dialect, drivers in _drivers().items():
        types = [*cases[dialect].items()]
        types += [(sql_type, value) for sql_type, value in EXTRA[dialect].items()]
        for sql_type, value in types:
            observed: dict[str, str] = {}
            for name, fetch in drivers.items():
                observed[name] = _observe(fetch, sql_type, value)
            reference = observed.get(REFERENCE[dialect])
            expected = None if reference is None or reference.startswith("error:") else reference
            provenance = ", ".join(f"{name}: {seen}" for name, seen in observed.items())
            yield Row("drivers", "live", dialect, "column", sql_type, expected, provenance)


def _observe(fetch: Fetch, sql_type: str, value: str) -> str:
    try:
        result = fetch(sql_type, value)
    except Exception as exc:  # any driver's error is a finding
        return f"error: {type(exc).__name__}"
    return _name(type(result))


def _name(cls: type) -> str:
    module = cls.__module__
    return cls.__name__ if module == "builtins" else f"{module}.{cls.__qualname__}"


def _drivers() -> dict[str, dict[str, Fetch]]:
    found: dict[str, dict[str, Fetch]] = {"sqlite": {"sqlite3": _sqlite3}}
    try:
        import aiosqlite  # noqa: F401
    except ImportError:
        pass
    else:
        found["sqlite"]["aiosqlite"] = _aiosqlite
    if dsn := os.environ.get("SQLTY_PG_DSN"):
        found["postgres"] = _postgres(dsn)
    if dsn := os.environ.get("SQLTY_MYSQL_DSN"):
        found["mysql"] = _mysql(dsn)
    return found


def _sqlite3(sql_type: str, value: str) -> Any:
    conn = sqlite3.connect(":memory:")
    conn.execute(f"CREATE TABLE t (c {sql_type})")
    conn.execute(f"INSERT INTO t VALUES ({value})")
    return conn.execute("SELECT c FROM t").fetchone()[0]


def _aiosqlite(sql_type: str, value: str) -> Any:
    import aiosqlite

    async def fetch() -> Any:
        async with aiosqlite.connect(":memory:") as conn:
            await conn.execute(f"CREATE TABLE t (c {sql_type})")
            await conn.execute(f"INSERT INTO t VALUES ({value})")
            async with conn.execute("SELECT c FROM t") as cursor:
                return (await cursor.fetchone())[0]

    return asyncio.run(fetch())


def _postgres(dsn: str) -> dict[str, Fetch]:
    def statements(sql_type: str, value: str) -> list[str]:
        return [
            "DROP TABLE IF EXISTS pg_temp.t",
            f"CREATE TEMP TABLE t (c {sql_type})",
            f"INSERT INTO t VALUES ({value})",
        ]

    def psycopg3(sql_type: str, value: str) -> Any:
        import psycopg

        with psycopg.connect(dsn, autocommit=True) as conn:
            for statement in statements(sql_type, value):
                conn.execute(statement)
            return conn.execute("SELECT c FROM t", prepare=False).fetchone()[0]

    def psycopg2(sql_type: str, value: str) -> Any:
        import psycopg2

        conn = psycopg2.connect(dsn)
        try:
            cursor = conn.cursor()
            for statement in statements(sql_type, value):
                cursor.execute(statement)
            cursor.execute("SELECT c FROM t")
            return cursor.fetchone()[0]
        finally:
            conn.close()

    def asyncpg_(sql_type: str, value: str) -> Any:
        import asyncpg

        async def fetch() -> Any:
            conn = await asyncpg.connect(dsn)
            try:
                for statement in statements(sql_type, value):
                    await conn.execute(statement)
                return await conn.fetchval("SELECT c FROM t")
            finally:
                await conn.close()

        return asyncio.run(fetch())

    drivers: dict[str, Fetch] = {"psycopg": psycopg3}
    for name, fetch, module in (
        ("psycopg2", psycopg2, "psycopg2"),
        ("asyncpg", asyncpg_, "asyncpg"),
    ):
        try:
            __import__(module)
        except ImportError:
            continue
        drivers[name] = fetch
    return drivers


def _mysql(dsn: str) -> dict[str, Fetch]:
    url = urlparse(dsn)
    options = {key: values[0] for key, values in parse_qs(url.query).items()}
    params = {
        "host": url.hostname or "localhost",
        "user": url.username or "root",
        "password": url.password or "",
        "database": url.path.lstrip("/"),
        **({"port": url.port} if url.port else {}),
        **options,
    }

    def fetcher(module: str) -> Fetch:
        def fetch(sql_type: str, value: str) -> Any:
            connect = __import__(module).connect
            kwargs = dict(params)
            if module == "MySQLdb":
                kwargs["db"] = kwargs.pop("database")
                kwargs["passwd"] = kwargs.pop("password")
            conn = connect(**kwargs)
            try:
                cursor = conn.cursor()
                cursor.execute("SET SESSION sql_mode = ''")
                cursor.execute("DROP TEMPORARY TABLE IF EXISTS t")
                cursor.execute(f"CREATE TEMPORARY TABLE t (c {sql_type})")
                cursor.execute(f"INSERT INTO t VALUES ({value})")
                cursor.execute("SELECT c FROM t")
                return cursor.fetchone()[0]
            finally:
                conn.close()

        return fetch

    drivers: dict[str, Fetch] = {}
    for name, module in (("pymysql", "pymysql"), ("mysqlclient", "MySQLdb")):
        try:
            __import__(module)
        except ImportError:
            continue
        drivers[name] = fetcher(module)
    return drivers
