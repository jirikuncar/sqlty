from typing import Literal, overload

@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name\n"],
) -> tuple[int, str]: ...
@overload
def sql(
    query: Literal["\n    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id\n"],
) -> tuple[int, str, str]: ...
@overload
def sql(
    query: Literal["SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id"],
) -> tuple[int, str, str]: ...
@overload
def sql(query: str) -> str: ...
