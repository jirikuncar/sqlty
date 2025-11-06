"""Post-related queries registry."""

from typing import Any

from sqlty import sqlty


@sqlty
def post_sql(query: str) -> Any:
    return query


# Define queries in the same file as the registry
get_post = post_sql("""
    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE id = :id AND user_id = :user_id
""")

list_posts = post_sql("""
    SELECT id::INTEGER, title::TEXT FROM posts ORDER BY created_at DESC
""")
