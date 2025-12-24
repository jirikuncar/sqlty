from typing import Literal, overload

@overload
def sql_a(
    query: Literal["\n    SELECT id, name, age FROM users WHERE id = :id\n    "],
) -> tuple[int, str, int]: ...
@overload
def sql_a(query: str) -> str: ...
@overload
def sql_b(
    query: Literal["\n    SELECT id, name, age FROM users WHERE id = :id\n    "],
) -> tuple[int, str, float]: ...
@overload
def sql_b(query: str) -> str: ...
