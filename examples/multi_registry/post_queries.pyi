from typing import Literal, overload

@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, title::TEXT FROM posts ORDER BY created_at DESC\n"],
) -> tuple[int, str]: ...
@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id\n"],
) -> tuple[int, str]: ...
@overload
def sql(
    query: Literal[
        "\n    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE id = :id AND user_id = :user_id\n"
    ],
) -> tuple[int, str, str]: ...
@overload
def sql(
    query: Literal["SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id"],
) -> tuple[int, str]: ...
@overload
def sql(query: str) -> str: ...
