from typing import Literal, overload

@overload
def sql(
    query: Literal["SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id"],
) -> tuple[int, str, str]: ...
@overload
def sql(query: str) -> str: ...
