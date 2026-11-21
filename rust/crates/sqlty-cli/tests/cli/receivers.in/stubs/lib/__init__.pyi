from typing import Annotated, LiteralString

from sqlty import SQL

class Db:
    def one[T](self, sql: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...
