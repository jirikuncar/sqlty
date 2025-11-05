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

from sqlty import SQLRegistry


# Example 1: Single Registry
print("=== Example 1: Single Registry ===")


class UserQueries(SQLRegistry):
    """User-related queries."""

    pass


user_sql = UserQueries.sql

# These queries will be automatically added to the stub
user_by_id = user_sql("SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id")
all_users = user_sql("SELECT id::INTEGER, name::TEXT FROM users")
user_count = user_sql("SELECT COUNT(*)::INTEGER FROM users")

print(f"✓ User queries registered: {len([user_by_id, all_users, user_count])}")


# Example 2: Multiple Registries
print("\n=== Example 2: Multiple Registries ===")


class PostQueries(SQLRegistry):
    """Post-related queries."""

    pass


class CommentQueries(SQLRegistry):
    """Comment-related queries."""

    pass


post_sql = PostQueries.sql
comment_sql = CommentQueries.sql

# Queries for different registries
post_by_id = post_sql("SELECT id::INTEGER, title::TEXT, body::TEXT FROM posts WHERE id = :id")
posts_by_user = post_sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id")
post_count = post_sql("SELECT COUNT(*)::INTEGER FROM posts")

comments_by_post = comment_sql(
    "SELECT id::INTEGER, text::TEXT FROM comments WHERE post_id = :post_id"
)
comment_count = comment_sql("SELECT COUNT(*)::INTEGER FROM comments")

print(f"✓ Post queries registered: 3")
print(f"✓ Comment queries registered: 2")


# Summary
print("\n=== Summary ===")
print("When this script exits, the following stub files will be created/updated:")
print("  - demo.pyi (with all UserQueries, PostQueries, and CommentQueries)")
print("\nAll queries will have proper type annotations based on SQL type casts.")
print("\nYou can verify by checking the generated .pyi file after running this script.")
