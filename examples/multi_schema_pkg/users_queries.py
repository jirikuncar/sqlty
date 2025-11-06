"""User queries using @sqlty decorator."""

from typing import Any

from sqlty import sqlty


@sqlty(schema="users.sql")
def sql(query: str) -> Any:
    """Execute user-related SQL queries."""
    return query


user_query = sql("SELECT id, name FROM users")
