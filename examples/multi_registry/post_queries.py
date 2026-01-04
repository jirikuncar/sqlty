"""Post-related queries registry.

Functions are registered in pyproject.toml rather than using decorators.
This allows SQLTy to properly track which file each query belongs to.
"""

from typing import Any


def sql(query: str) -> Any:
    """Execute post queries (registered in pyproject.toml)."""
    return query


# Define queries in the same file as the registry
get_post = sql("""
    SELECT id::INTEGER, title::TEXT FROM posts WHERE id = :id
""")

list_posts = sql("""
    SELECT id::INTEGER, title::TEXT FROM posts ORDER BY created_at DESC
""")
