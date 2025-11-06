"""Application that uses multiple registries."""

from .post_queries import get_post, get_user_posts, list_posts
from .user_queries import get_user, get_user_with_email, list_users


def get_user_data(user_id: int) -> tuple[object, object]:
    """Get user and their posts."""
    # Using queries from UserQueries registry
    user = get_user_with_email

    # Using queries from PostQueries registry
    posts = get_user_posts

    return user, posts
