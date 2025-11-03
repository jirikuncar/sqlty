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

from .queries import Query as Q

query = Q.sql(
    """
    SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id
    """
)


def main() -> None:
    conn = Connection.connect("dbname=test user=postgres password=secret host=localhost port=5432")

    params: Params = {"id": 1}

    rows: Iterator[tuple[int, str]] = execute(conn, query, params)

    for row in rows:
        print(f"User ID: {row[0]}, Name: {row[1]}")

    conn.close()
