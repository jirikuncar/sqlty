from typing import Annotated, LiteralString

from sqlty import SQL


class Store:
    def one[T](self, sql: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...


class Cached(Store):
    pass
