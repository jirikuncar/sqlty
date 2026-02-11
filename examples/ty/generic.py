from typing import Protocol, TypedDict, cast, runtime_checkable

from sqlty import SQL


def foo[T: P, P](x: SQL[P]) -> T:
    return cast(T, x)

class User(TypedDict):
    id: int
    name: str


@runtime_checkable
class UserProtocol(Protocol):
    @property
    def id(self) -> int: ...
    @property
    def name(self) -> str: ...


x: User = foo(SQL[UserProtocol]("SELECT id::INTEGER, name::TEXT FROM users"))


class NotUser(TypedDict):
    id: int
    fullname: str

@runtime_checkable
class NotUserProtocol(Protocol):
    @property
    def id(self) -> int: ...
    @property
    def fullname(self) -> str: ...


y: User = foo(SQL[NotUser]("SELECT id::INTEGER, fullname::TEXT FROM users"))


from ty_extensions import Intersection, Not, is_assignable_to, is_subtype_of, static_assert, is_gradual_equivalent_to


static_assert(is_gradual_equivalent_to(User, UserProtocol))
static_assert(is_gradual_equivalent_to(UserProtocol, User))

# def h[T, P](query: SQL[T], tp: P) -> P:
#     static_assert(is_gradual_equivalent_to(T, P))
#     return cast(P, query)


# z: User = h(SQL[UserProtocol]("SELECT id::INTEGER, name::TEXT FROM users"), User)
