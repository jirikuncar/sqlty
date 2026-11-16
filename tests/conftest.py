"""Each pytest-xdist worker gets its own PostgreSQL database, so tests that reset it
(``test_pg_describe`` drops every schema) run in parallel."""

from __future__ import annotations

import contextlib
import os

import pytest

WORKER = os.environ.get("PYTEST_XDIST_WORKER")

# Coverage must not depend on what Hypothesis happens to generate.
with contextlib.suppress(ImportError):
    from hypothesis import Phase, settings

    settings.register_profile("coverage", phases=[Phase.explicit])


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
