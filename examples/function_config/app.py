"""Example application using configured functions.

This example demonstrates how to use SQLTy with functions configured
in pyproject.toml rather than using @sqlty decorators.
"""

from db import execute_query, pg_fetchall, pg_fetchvalue

# =============================================================================
# POSITIVE EXAMPLES: Correct type usage
# =============================================================================

# Example 1: Integer query with matching return_tp
user_id: int | None = pg_fetchvalue(
    None,
    query="SELECT id::INTEGER FROM users WHERE email = :email",
    return_tp=int,
    email="user@example.com",
)

# Example 2: Count query
count: int | None = pg_fetchvalue(
    None,
    query="SELECT COUNT(*)::INTEGER FROM users",
    return_tp=int,
)

# Example 3: String query with matching return_tp
user_name: str | None = pg_fetchvalue(
    None,
    query="SELECT name::TEXT FROM users WHERE id = :id",
    return_tp=str,
    id=1,
)

# Example 4: Fetch all values
user_ids: list[int] = pg_fetchall(
    None,
    query="SELECT id::INTEGER FROM users",
    return_tp=int,
)

# Example 5: Function without return_type_param (using schema for inference)
# Type inference happens based on the SQL query and schema
results = execute_query(
    query="SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id",
    id=1,
)

# Example 6: Boolean query with schema
published_posts = execute_query(
    query="SELECT title::TEXT FROM posts WHERE published = :published",
    published=True,
)


# =============================================================================
# NEGATIVE EXAMPLES: Type mismatches that mypy should catch
# =============================================================================

# ERROR: return_tp=float but query returns TEXT
# This should fail type checking because name is TEXT, not FLOAT
# name_as_float: float | None = pg_fetchvalue(
#     None,
#     query="SELECT name::TEXT FROM users LIMIT 1",
#     return_tp=float,  # type: ignore[arg-type]  # Wrong type!
# )

# ERROR: return_tp=str but query returns INTEGER
# count_as_str: str | None = pg_fetchvalue(
#     None,
#     query="SELECT COUNT(*)::INTEGER FROM users",
#     return_tp=str,  # type: ignore[arg-type]  # Wrong type!
# )

# ERROR: Assigning result to wrong type
# wrong_annotation: str | None = pg_fetchvalue(
#     None,
#     query="SELECT COUNT(*)::INTEGER FROM users",
#     return_tp=int,  # type: ignore[assignment]  # Returns int, not str!
# )


def main() -> None:
    """Main function demonstrating usage."""
    print("This example demonstrates SQLTy function configuration.")
    print("Run 'sqlty .' to generate type stubs for db.py")
    print("Then run 'mypy app.py' to see type checking in action!")


if __name__ == "__main__":
    main()
