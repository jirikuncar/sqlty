from typing import Literal, overload
from sqlty import SQL

@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, title::TEXT FROM posts ORDER BY created_at DESC\n"],
) -> SQL[tuple[int, str]]: ...
@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, title::TEXT FROM posts WHERE id = :id\n"],
) -> SQL[tuple[int, str]]: ...
@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id\n"],
) -> SQL[tuple[int, str]]: ...
@overload
def sql(query: str) -> str: ...
