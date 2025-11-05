from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class CommentQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT COUNT(*)::INTEGER FROM comments']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, text::TEXT FROM comments WHERE post_id = :post_id']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...

class PostQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT COUNT(*)::INTEGER FROM posts']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, title::TEXT, body::TEXT FROM posts WHERE id = :id']) -> SQL[tuple[int, str, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...

class UserQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT COUNT(*)::INTEGER FROM users']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, name::TEXT FROM users']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id']) -> SQL[tuple[int, str, str]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::TEXT, email::TEXT, created_at::TEXT FROM users']) -> SQL[tuple[str, str, str]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::TEXT, name::TEXT FROM users']) -> SQL[tuple[str, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
comment_sql = CommentQueries.sql
post_sql = PostQueries.sql
user_sql = UserQueries.sql
