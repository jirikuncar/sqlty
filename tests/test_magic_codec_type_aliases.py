"""Test that magic codec works with type aliases for human-friendly names."""

from collections import namedtuple
from typing import TYPE_CHECKING, Any, Final, Generic, LiteralString, NamedTuple, Protocol, Type, TypeAlias, TypeVar, cast, get_type_hints, overload, runtime_checkable, runtime_checkable

import pytest

from sqlty import SQL, Callable


def test_namedtuple_type_alias_compatibility() -> None:
    """Test that namedtuples with same structure are compatible."""
    class QueryRow_724392cb(NamedTuple):
        id: int
        name: str

    class User(Protocol):
        @property
        def id(self) -> int: ...
        @property
        def name(self) -> str: ...

    # They should be compatible for assignment
    query_row_instance = QueryRow_724392cb(1, "Alice")
    user_instance: User = query_row_instance

    assert user_instance.id == 1
    assert user_instance.name == "Alice"

    query: SQL[User] = SQL[QueryRow_724392cb]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
    assert type(query) is SQL


def create_query[T](return_type: type[T], raw_sql: str) -> SQL[T]:
    """
    Given a class T, returns a covariant SQL[T] object.
    """
    # We cast to SQL[T] to satisfy the type checker 
    # that the string is now bound to this type.
    return SQL[T](raw_sql)


def pg_fetchone[T](
    conn: Any,
    return_cls: type[T], # The user provides a specific class (NamedTuple/Dataclass)
    query: SQL[T],      # The query expects a specific structure (Protocol)
    /,
    _trace_span: bool = True,
    **params: Any,
) -> T | None:
    ...


P = TypeVar("P") 

def pg_fetchone_ng[T: P, P](
    return_cls: type[T], 
    query: SQL[P], 
    **params: Any
) -> T | None:
    """
    T: P ensures that the return_cls (T) must satisfy 
    the structure of the query's protocol (P).
    """
    # ... runtime implementation (filtering etc.) ...
    return None


def test_namedtuple_with_protocol() -> None:
    """Test that namedtuples satisfy Protocol structural typing."""
    from typing import TypeVar, Protocol, NamedTuple, Generic

    # 1. Define the structural requirement
    @runtime_checkable
    class UserShape(Protocol):
        @property
        def id(self) -> int: ...
        @property
        def name(self) -> str: ...

    # 2. Define a TypeVar bounded by that Protocol
    # This means: "T can be any class, as long as it matches UserShape"
    T_user = TypeVar("T_user", bound=UserShape)

    # 3. Use this TypeVar in your function
    def process_user_record(query: SQL[T_user]) -> T_user:
        """
        Accepts SQL[User], SQL[Customer], or SQL[GeneratedRow].
        Returns the exact type passed in.
        """
        print(f"Processing record for query: {query}")
        # ... execution logic ...
        return cast(T_user, ...)

    @runtime_checkable
    class QueryRow_724392cb(Protocol):
        @property
        def id(self) -> int: ...
        @property
        def name(self) -> str: ...

    class _QueryRow_724392cb(NamedTuple):
        id: int
        name: str

    query =  SQL[QueryRow_724392cb]("--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")

    class User(NamedTuple):
        id: int
        name: str

    result = process_user_record(query)
    assert isinstance(result, QueryRow_724392cb)
