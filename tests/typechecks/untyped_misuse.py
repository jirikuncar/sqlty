"""Using an SQL[UNTYPED] row as a concrete type is an error in every checker."""

from sqlty import SQL, UNTYPED


def rows[T](query: SQL[T]) -> list[T]:
    return []


row: tuple[int] = rows(SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs"))[0]
