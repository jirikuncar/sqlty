from typing import Literal, overload

@overload
def sql(query: Literal["\n    SELECT COUNT(*)::INTEGER FROM users\n    "]) -> tuple[int]: ...
@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id\n    "],
) -> tuple[int, str]: ...
@overload
def sql(
    query: Literal[
        "\n    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE user_id = :user_id\n    "
    ],
) -> tuple[int, str, str]: ...
@overload
def sql(query: str) -> str: ...
