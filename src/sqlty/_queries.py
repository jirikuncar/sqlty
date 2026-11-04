"""Named queries in ``.sql`` files (aiosql, sqlc, PugSQL): checked by the engine, suppressed here."""

from __future__ import annotations

import re

from sqlty._check import apply_suppressions
from sqlty._core import Schema, analyze_params, infer_type, returns_rows
from sqlty._core import check_queries as _check_queries
from sqlty._core import named_queries as _named_queries
from sqlty._lint import Diagnostic
from sqlty._source import Ignore

_IGNORE_RE = re.compile(rb"--\s*sqlty:\s*ignore\b(?:\[([^\]]*)\])?")


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
    found = _check_queries(
        source,
        dialect=dialect,
        schema_sql=schema_sql,
        rules=rules,
        query_format=query_format,
        name=name,
    )
    ignores: dict[int, Ignore] = {}
    if b"sqlty:" in (data := source.encode()):
        offset = 0
        for number, line in enumerate(data.splitlines(keepends=True), start=1):
            if match := _IGNORE_RE.search(line):
                names = match.group(1)
                rules_named = (
                    None
                    if names is None
                    else tuple(n.strip() for n in names.decode().split(",") if n.strip())
                )
                ignores[number] = Ignore(
                    number, rules_named, offset + match.start(), offset + match.end()
                )
            offset += len(line)
    return apply_suppressions(ignores, found, rules, marker="--")


def query_types(
    source: str,
    *,
    name: str = "",
    query_format: str = "auto",
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> list[str]:
    """``name(params) -> row`` for each named query of a ``.sql`` file, as ``sqlty check -v``
    prints them: the parameters by name or number, ``None`` for a query without rows and
    ``unknown`` for one whose columns are not all typed. A query of several statements is
    typed by its first.

    Example:
        >>> query_types("-- name: by-id^\\nSELECT id FROM users WHERE id = :id", schema_sql="CREATE TABLE users (id INT NOT NULL)")
        ['by_id(id: int) -> tuple[int]']
    """
    data = source.encode()
    found = []
    for query, start, end in _named_queries(source, query_format=query_format, name=name):
        body = data[start:end].decode()
        row = infer_type(body, dialect=dialect, schema_sql=schema_sql)
        if row is None and returns_rows(body, dialect=dialect):
            row = "unknown"
        params = analyze_params(body, dialect=dialect, schema_sql=schema_sql) or []
        listed = ", ".join(
            f"{p[1] or f'${p[0]}'}: {p[2] or 'Any'}" for p in sorted(params, key=lambda p: p[0])
        )
        found.append(f"{query}({listed}) -> {row}")
    return found


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
