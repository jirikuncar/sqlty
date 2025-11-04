import uuid
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class SchemaQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT id, name FROM users"]) -> SQL[tuple[int, str]]: ...
    @overload
    @classmethod
    def sql(
        cls, query: Literal["SELECT id, user_id, name FROM users"]
    ) -> SQL[tuple[int, uuid.UUID, str]]: ...
    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = SchemaQueries.sql
