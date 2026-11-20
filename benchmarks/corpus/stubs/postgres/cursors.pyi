from typing import Annotated, Any, LiteralString

from sqlty import SQL

class SimpleCursorBase:
    def run(
        self, sql: Annotated[LiteralString, SQL], parameters: Any = None, **kw: Any
    ) -> None: ...
    def one(
        self,
        sql: Annotated[LiteralString, SQL],
        parameters: Any = None,
        default: Any = None,
        back_as: Any = None,
        max_age: Any = None,
        **kw: Any,
    ) -> Any: ...
    def all(
        self,
        sql: Annotated[LiteralString, SQL],
        parameters: Any = None,
        back_as: Any = None,
        max_age: Any = None,
        **kw: Any,
    ) -> list[Any]: ...
