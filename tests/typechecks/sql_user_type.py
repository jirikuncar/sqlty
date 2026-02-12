"""SQL[T] with user-defined types (TypedDict, NamedTuple, dataclass).

mypy: ok
pyright: ok
ty: ok
"""

from typing import NamedTuple, TypedDict
from dataclasses import dataclass

from sqlty import SQL


class UserTD(TypedDict):
    id: int
    name: str


class UserNT(NamedTuple):
    id: int
    name: str


@dataclass
class UserDC:
    id: int
    name: str


# All type forms are accepted as type parameters
td_query = SQL[UserTD]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
nt_query = SQL[UserNT]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
dc_query = SQL[UserDC]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")

reveal_type(td_query)  # SQL[UserTD]
reveal_type(nt_query)  # SQL[UserNT]
reveal_type(dc_query)  # SQL[UserDC]
