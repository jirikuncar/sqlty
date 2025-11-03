"""User-related queries registry."""

from sqlty import SQLRegistry


class UserQueries(SQLRegistry):
    """Registry for user-related SQL queries."""

    pass


# Export the sql method
user_sql = UserQueries.sql


# Define queries in the same file as the registry
get_user = user_sql("""
    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id
""")

list_users = user_sql("""
    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name
""")
