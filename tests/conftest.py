"""Each pytest-xdist worker gets its own PostgreSQL database, so tests that reset it
(``test_pg_describe`` drops every schema) run in parallel."""

from __future__ import annotations

import contextlib
import os
from typing import Any
from urllib.parse import parse_qs, urlparse

import pytest

WORKER = os.environ.get("PYTEST_XDIST_WORKER")

# Tests run their explicit examples only, so neither coverage nor the suite's time depends on
# what Hypothesis generates; the Fuzz workflow generates with `--hypothesis-profile=fuzz`.
with contextlib.suppress(ImportError):
    from hypothesis import Phase, settings

    settings.register_profile("explicit", phases=[Phase.explicit])
    settings.register_profile("fuzz")
    settings.load_profile("explicit")


def _worker_database(dsn: str, *, create: bool) -> str:
    import psycopg
    from psycopg import sql
    from psycopg.conninfo import conninfo_to_dict, make_conninfo

    name = f"{conninfo_to_dict(dsn).get('dbname') or 'postgres'}_{WORKER}"
    with psycopg.connect(dsn, autocommit=True) as conn:
        conn.execute(
            sql.SQL("DROP DATABASE IF EXISTS {} WITH (FORCE)").format(sql.Identifier(name))
        )
        if create:
            conn.execute(sql.SQL("CREATE DATABASE {}").format(sql.Identifier(name)))
    return make_conninfo(dsn, dbname=name)


def pytest_configure(config: pytest.Config) -> None:
    if WORKER and (dsn := os.environ.get("SQLTY_PG_DSN")):
        os.environ["SQLTY_PG_SERVER_DSN"] = dsn
        os.environ["SQLTY_PG_DSN"] = _worker_database(dsn, create=True)


def pytest_unconfigure(config: pytest.Config) -> None:
    if WORKER and (dsn := os.environ.get("SQLTY_PG_SERVER_DSN")):
        _worker_database(dsn, create=False)


@pytest.fixture(scope="session")
def mysql_connect_args() -> dict[str, Any]:
    """PyMySQL's and rustmy's ``connect`` arguments for ``SQLTY_MYSQL_DSN``
    (``mysql://user:password@host:port/database`` or ``unix_socket=...``)."""
    dsn = os.environ.get("SQLTY_MYSQL_DSN")
    if not dsn:
        pytest.skip("SQLTY_MYSQL_DSN not set")
    url = urlparse(dsn)
    return {
        "host": url.hostname or "localhost",
        "port": url.port or 3306,
        "user": url.username or "root",
        "password": url.password or "",
        "database": url.path.lstrip("/"),
        "autocommit": True,
        **{key: values[0] for key, values in parse_qs(url.query).items()},
    }
