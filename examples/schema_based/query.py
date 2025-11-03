from collections.abc import Iterator

from sqlty import SQLRegistry


class SchemaQueries(SQLRegistry):
    __schema_path__ = "schema.sql"  # Relative to this file


sql = SchemaQueries.sql


# Query using table defined in schema.sql (auto-detected via __schema_path__)
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
