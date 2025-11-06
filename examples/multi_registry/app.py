from .post_queries import sql as post_sql
from .user_queries import sql as user_sql


def get_user_data(user_id: int) -> tuple[SQL[tuple[int, str, str]], SQL[int, str]]:
    """Get user and their posts."""
    # Using queries from UserQueries registry
    user = user_sql("SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id")

    # Using queries from PostQueries registry
    posts = post_sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id")

    return user, posts
