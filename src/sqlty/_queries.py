"""Named queries in ``.sql`` files (aiosql, sqlc, PugSQL), checked by the engine."""

from __future__ import annotations

from sqlty._core import Schema
from sqlty._core import check_queries as _check_queries
from sqlty._lint import Diagnostic


def check_query_file(
    source: str,
    *,
    name: str = "",
    query_format: str = "auto",
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    rules: dict[str, str] | None = None,
) -> list[Diagnostic]:
    """``sqlty check`` of a ``.sql`` file of named queries; ``-- sqlty: ignore[rule]`` silences a line.

    Example:
        >>> [d["message"] for d in check_query_file("-- name: one^\\nSELECT nope FROM users", schema_sql="CREATE TABLE users (id INT)")]
        ['column `nope` does not exist in `users`']
    """
    return _check_queries(
        source,
        dialect=dialect,
        schema_sql=schema_sql,
        rules=rules,
        query_format=query_format,
        name=name,
    )


def breaking_query_file(
    source: str,
    *,
    schema_sql: str | Schema | None,
    against_sql: str | Schema | None,
    name: str = "",
    query_format: str = "auto",
    dialect: str = "postgres",
    rules: dict[str, str] | None = None,
) -> list[Diagnostic]:
    """The diagnostics ``source`` gets against ``against_sql`` but not against its own
    ``schema_sql``, as ``breaking_source`` finds them for Python.

    Example:
        >>> old, new = "CREATE TABLE users (id INT, bio TEXT)", "CREATE TABLE users (id INT)"
        >>> [d["message"] for d in breaking_query_file("SELECT bio FROM users", schema_sql=old, against_sql=new)]
        ['column `bio` does not exist in `users`']
    """

    def check(schema: str | Schema | None) -> list[Diagnostic]:
        return check_query_file(
            source,
            name=name,
            query_format=query_format,
            dialect=dialect,
            schema_sql=schema,
            rules=rules,
        )

    own = {(d["line"], d["rule"], d["message"]) for d in check(schema_sql)}
    return [
        d
        for d in check(against_sql)
        if (d["line"], d["rule"], d["message"]) not in own and d["rule"] != "unused-ignore"
    ]
