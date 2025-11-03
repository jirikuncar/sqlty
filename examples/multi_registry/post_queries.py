"""Post-related queries registry."""

from sqlty import SQLRegistry


class PostQueries(SQLRegistry):
    """Registry for post-related SQL queries."""

    pass


# Export the sql method
post_sql = PostQueries.sql


# Define queries in the same file as the registry
get_post = post_sql("""
    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE id = :id
""")

list_posts = post_sql("""
    SELECT id::INTEGER, title::TEXT FROM posts ORDER BY created_at DESC
""")
