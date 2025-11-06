from typing import Literal, overload
from sqlty import SQL

@overload
def sql(query: Literal["SELECT id, total FROM orders"]) -> SQL[tuple[int, float]]: ...
@overload
def sql(query: str) -> str: ...
