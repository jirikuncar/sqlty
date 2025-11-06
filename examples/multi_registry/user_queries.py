"""User-related queries registry."""

from typing import Any

from sqlty import sqlty


@sqlty
def sql(query: str) -> Any:
    return query


# Define queries in the same file as the registry
get_user = sql("""
    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id
""")

list_users = sql("""
    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name
""")

get_user_with_email = sql("""
    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id
""")
