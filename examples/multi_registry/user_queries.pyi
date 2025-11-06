from typing import Literal, overload
from sqlty import SQL

@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name\n"],
) -> SQL[tuple[int, str]]: ...
@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id\n"],
) -> SQL[tuple[int, str, str]]: ...
@overload
def sql(query: str) -> str: ...
