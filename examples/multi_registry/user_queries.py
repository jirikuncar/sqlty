"""User-related queries registry."""

from typing import Any

from sqlty import sqlty


@sqlty
def user_sql(query: str) -> Any:
    return query


# Define queries in the same file as the registry
get_user = user_sql("""
    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id
""")

list_users = user_sql("""
    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name
""")
