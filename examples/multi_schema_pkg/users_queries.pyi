from typing import Literal, overload
from sqlty import SQL

@overload
def sql(query: Literal["SELECT id, name FROM users"]) -> SQL[tuple[int, str]]: ...
@overload
def sql(query: str) -> str: ...
