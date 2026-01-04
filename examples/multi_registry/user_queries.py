"""User-related queries registry.

Functions are registered in pyproject.toml rather than using decorators.
This allows SQLTy to properly track which file each query belongs to.
"""

from typing import Any


def sql(query: str) -> Any:
    """Execute user queries (registered in pyproject.toml)."""
    return query


# Define queries in the same file as the registry
get_user = sql("""
    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id
""")

list_users = sql("""
    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name
""")
