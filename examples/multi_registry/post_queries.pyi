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
def sql(
    query: Literal["\n    SELECT id::INTEGER, title::TEXT FROM posts ORDER BY created_at DESC\n"],
) -> SQL[tuple[int, str]]: ...
@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id\n"],
) -> SQL[tuple[int, str]]: ...
@overload
def sql(
    query: Literal[
        "\n    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE id = :id AND user_id = :user_id\n"
    ],
) -> SQL[tuple[int, str, str]]: ...
@overload
def sql(
    query: Literal["SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id"],
) -> tuple[int, str]: ...
@overload
def sql(query: str) -> str: ...
