from typing import Literal, overload
from sqlty import SQL

@overload
def sql(query: Literal["\n    SELECT COUNT(*)::INTEGER FROM users\n    "]) -> SQL[tuple[int]]: ...
@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id\n    "],
) -> SQL[tuple[int, str]]: ...
@overload
def sql(
    query: Literal[
        "\n    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE user_id = :user_id\n    "
    ],
) -> SQL[tuple[int, str, str]]: ...
@overload
def sql(query: str) -> str: ...
