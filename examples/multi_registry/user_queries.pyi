from typing import Literal, overload
from sqlty import SQL

@overload
def user_sql(
    query: Literal[
        "\n        SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id ORDER BY name\n    "
    ],
) -> SQL[tuple[int, str, str]]: ...
@overload
def user_sql(
    query: Literal["\n    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name\n"],
) -> SQL[tuple[int, str]]: ...
@overload
def user_sql(
    query: Literal["\n    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id\n"],
) -> SQL[tuple[int, str, str]]: ...
@overload
def user_sql(query: str) -> str: ...
