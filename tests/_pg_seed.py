"""Rows for the tables of a PostgreSQL schema, to run queries on.

Each table gets rows with NULL in every nullable column, and rows with a value in every
column, so outer joins, aggregates and nullable columns can produce NULLs. A value is
the first of a few literals the column's type accepts; a table whose rows the server
rejects (a CHECK, a foreign key to a table without rows) stays empty.
"""

from __future__ import annotations

from typing import Any

ROWS = 2


def seed(conn: Any) -> None:
    import psycopg

    tables = conn.execute(
        "SELECT c.oid, format('%I.%I', n.nspname, c.relname) FROM pg_class c "
        "JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind IN ('r', 'p') "
        "AND n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema'"
    ).fetchall()
    values: dict[tuple[int, int, int], str | None] = {}

    def value(type_oid: int, typmod: int, k: int) -> str | None:
        if (type_oid, typmod, k) not in values:
            values[type_oid, typmod, k] = _value(conn, type_oid, typmod, k)
        return values[type_oid, typmod, k]

    pending: list[tuple[str, list[tuple[str, str]]]] = []
    for oid, table in tables:
        columns = conn.execute(
            "SELECT quote_ident(attname), format_type(atttypid, atttypmod), atttypid, "
            "atttypmod, attnotnull FROM pg_attribute WHERE attrelid = %s AND attnum > 0 "
            "AND NOT attisdropped AND NOT atthasdef AND attidentity = '' "
            "AND attgenerated = '' ORDER BY attnum",
            (oid,),
        ).fetchall()
        for k in range(1, 2 * ROWS + 1):
            with_nulls = k <= ROWS
            row = [
                (name, f"%s::{type_name}", value(type_oid, typmod, k))
                for name, type_name, type_oid, typmod, not_null in columns
                if not_null or not with_nulls
            ]
            names = ", ".join(name for name, _, _ in row)
            placeholders = ", ".join(placeholder for _, placeholder, _ in row)
            sql = (
                f"INSERT INTO {table} ({names}) VALUES ({placeholders})"
                if row
                else f"INSERT INTO {table} DEFAULT VALUES"
            )
            pending.append((sql, [literal for _, _, literal in row]))
    # A row whose foreign key needs another table's rows is retried after them.
    while pending:
        rejected = []
        for sql, params in pending:
            try:
                with conn.transaction():
                    conn.execute(sql, params)
            except psycopg.Error:
                rejected.append((sql, params))
        if len(rejected) == len(pending):
            return
        pending = rejected


def _value(conn: Any, type_oid: int, typmod: int, k: int) -> str | None:
    import psycopg

    labels = conn.execute(
        "SELECT enumlabel FROM pg_enum WHERE enumtypid = %s ORDER BY enumsortorder", (type_oid,)
    ).fetchall()
    if labels:
        return str(labels[(k - 1) % len(labels)][0])
    type_name = conn.execute("SELECT format_type(%s, %s)", (type_oid, typmod)).fetchone()[0]
    for literal in (
        str(k),
        "t" if k % 2 else "f",
        f"2024-01-0{k}",
        f"0{k}:00:00",
        f"10.0.0.{k}",
        f"00000000-0000-0000-0000-00000000000{k}",
        f"08:00:2b:01:02:0{k}",
        f"[{k},{k + 1})",
        f"({k},{k})",
        "{}",
        f"v{k}",
    ):
        try:
            with conn.transaction():
                conn.execute(f"SELECT %s::{type_name}", (literal,))
        except psycopg.Error:
            continue
        return literal
    return None
