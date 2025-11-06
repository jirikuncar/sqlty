"""Post-related queries registry."""

from typing import Any

from sqlty import sqlty


@sqlty
def sql(query: str) -> Any:
    return query


# Define queries in the same file as the registry
get_post = sql("""
    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE id = :id AND user_id = :user_id
""")

list_posts = sql("""
    SELECT id::INTEGER, title::TEXT FROM posts ORDER BY created_at DESC
""")

get_user_posts = sql("""
    SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id
""")
