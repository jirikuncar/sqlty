"""postgres 4.0 (postgres.py), as far as sqlty reads it: the classes whose methods run a query."""

from typing import Annotated, Any, LiteralString

from sqlty import SQL

from .context_managers import ConnectionContextManager, CursorContextManager
from .cursors import SimpleCursorBase

class Postgres:
    def __init__(self, url: str = ..., **kw: Any) -> None: ...
    def run(
        self, sql: Annotated[LiteralString, SQL], parameters: Any = None, **kw: Any
    ) -> None: ...
    def one(self, sql: Annotated[LiteralString, SQL], parameters: Any = None, **kw: Any) -> Any: ...
    def all(
        self, sql: Annotated[LiteralString, SQL], parameters: Any = None, **kw: Any
    ) -> list[Any]: ...
    def get_cursor(
        self, cursor: SimpleCursorBase | None = None, **kw: Any
    ) -> CursorContextManager: ...
    def get_connection(self, **kw: Any) -> ConnectionContextManager: ...
