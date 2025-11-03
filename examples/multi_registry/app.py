"""Application that uses multiple registries."""

from .post_queries import post_sql
from .user_queries import user_sql


def get_user_data(user_id: int) -> tuple[object, object]:
    """Get user and their posts."""
    # Using user_sql from UserQueries registry
    user = user_sql("""
        SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id ORDER BY name
    """)

    # Using post_sql from PostQueries registry
    posts = post_sql("""
        SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id ORDER BY created_at DESC
    """)

    return user, posts
