# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Comprehensive example demonstrating runtime auto-generation features.

This example shows:
1. Single registry with multiple queries
2. Multiple registries in one file
3. Incremental stub generation

Note: Only literal string queries are supported for type stub generation.
"""

from queries import comment_sql, post_sql, user_sql

from sqlty import SQL

# These queries will be automatically added to the stub
user_by_id: SQL[tuple[int, str, str]] = user_sql(
    "SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id"
)
all_users = user_sql("SELECT id::INTEGER, name::TEXT FROM users")
user_count = user_sql("SELECT COUNT(*)::INTEGER FROM users")

print("✓ User queries registered: 3")


# Queries for different registries
post_by_id = post_sql("SELECT id::INTEGER, title::TEXT, body::TEXT FROM posts WHERE id = :id")
posts_by_user = post_sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id")
post_count = post_sql("SELECT COUNT(*)::INTEGER FROM posts")

comments_by_post = comment_sql(
    "SELECT id::INTEGER, text::TEXT FROM comments WHERE post_id = :post_id"
)
comment_count = comment_sql("SELECT COUNT(*)::INTEGER FROM comments")

print("✓ Post queries registered: 3")
print("✓ Comment queries registered: 2")
