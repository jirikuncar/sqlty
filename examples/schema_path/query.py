# /// script
# requires-python = ">=3.14"
# dependencies = [
#     "psycopg[binary]",
# ]
# ///
"""
Show case example for simple query operation using the Query registry pattern.
"""

from collections.abc import Iterator

from psycopg import Connection
from psycopg.abc import Params

from sqlty.drivers.psycopg import execute

from .queries import sql_a, sql_b

query_a = sql_a(
    """
    SELECT id, name, age FROM users WHERE id = :id
    """
)

query_b = sql_b(
    """
    SELECT id, name, age FROM users WHERE id = :id
    """
)


def main() -> None:
    conn = Connection.connect("dbname=test user=postgres password=secret host=localhost port=5432")

    params: Params = {"id": 1}

    rows_a: Iterator[tuple[int, str, int]] = execute(conn, query_a, params)
    rows_b: Iterator[tuple[int, str, float]] = execute(conn, query_b, params)

    print("Query A Results:", list(rows_a))
    print("Query B Results:", list(rows_b))

    conn.close()
