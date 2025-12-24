from typing import Literal, overload, Any

@overload
def execute_query(
    query: Literal["SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id"],
) -> list[tuple[int, str, str]]: ...
@overload
def execute_query(
    query: Literal["SELECT title::TEXT FROM posts WHERE published = :published"],
) -> list[tuple[str]]: ...
@overload
def execute_query(query: str) -> str: ...
@overload
def pg_fetchall(
    conn: Any,
    query: Literal["SELECT id::INTEGER FROM users"],
    return_tp: type[int] | None = None,
    /,
    **params: Any,
) -> list[int]: ...
@overload
def pg_fetchall(query: str) -> str: ...
@overload
def pg_fetchvalue(
    conn: Any,
    query: Literal["SELECT COUNT(*)::INTEGER FROM users"],
    return_tp: type[int] | None = None,
    /,
    **params: Any,
) -> int | None: ...
@overload
def pg_fetchvalue(
    conn: Any,
    query: Literal["SELECT id::INTEGER FROM users WHERE email = :email"],
    return_tp: type[int] | None = None,
    /,
    **params: Any,
) -> int | None: ...
@overload
def pg_fetchvalue(
    conn: Any,
    query: Literal["SELECT name::TEXT FROM users LIMIT 1"],
    return_tp: type[float] | None = None,
    /,
    **params: Any,
) -> float | None: ...
@overload
def pg_fetchvalue(
    conn: Any,
    query: Literal["SELECT name::TEXT FROM users WHERE id = :id"],
    return_tp: type[str] | None = None,
    /,
    **params: Any,
) -> str | None: ...
@overload
def pg_fetchvalue(query: str) -> str: ...
