from collections.abc import Iterator

from typing import Any

from sqlty import sqlty


@sqlty(schema="schema.sql")
def sql(query: str) -> Any:
    """Execute SQL queries with schema-based type inference."""
    return query


# Query using table defined in schema.sql (auto-detected via schema parameter)
user_query = sql("SELECT id, name FROM users")
# Type: SQL[tuple[int, str]]

# Query with UUID type from schema
user_with_uuid = sql("SELECT id, user_id, name FROM users")
# Type: SQL[tuple[int, uuid.UUID, str]]


def main() -> None:
    from psycopg import Connection
    from psycopg.abc import Params

    from sqlty.drivers.psycopg import execute

    conn = Connection.connect("dbname=test user=postgres password=secret host=localhost port=5432")

    params: Params = {"id": 1}

    rows: Iterator[tuple[int, str]] = execute(conn, user_query, params)

    for row in rows:
        print(f"User ID: {row[0]}, Name: {row[1]}")

    conn.close()
