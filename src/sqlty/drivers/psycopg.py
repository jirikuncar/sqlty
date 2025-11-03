from collections.abc import Iterator
from typing import cast

from psycopg import Connection
from psycopg.abc import Params

from sqlty import SQL


def execute[T](conn: Connection, query: SQL[T], params: Params) -> Iterator[T]:
    # Dummy implementation for demonstration purposes
    return cast(Iterator[T], conn.execute(query.encode(), params).fetchall())
