"""Test file to verify type checking works correctly.

This demonstrates both correct usage and type mismatches that should be caught.
"""

from db import pg_fetchall, pg_fetchvalue

# =============================================================================
# POSITIVE EXAMPLES: These should pass type checking
# =============================================================================

# Example 1: Correct type - INTEGER query with int return_tp
user_id: int | None = pg_fetchvalue(
    None,
    query="SELECT id::INTEGER FROM users WHERE email = :email",
    return_tp=int,
    email="user@example.com",
)

# Example 2: Correct type - TEXT query with str return_tp
user_name: str | None = pg_fetchvalue(
    None,
    query="SELECT name::TEXT FROM users WHERE id = :id",
    return_tp=str,
    id=1,
)

# Example 3: Correct type - fetchall with int
user_ids: list[int] = pg_fetchall(
    None,
    query="SELECT id::INTEGER FROM users",
    return_tp=int,
)

# =============================================================================
# NEGATIVE EXAMPLES: These should FAIL type checking
# =============================================================================

# ERROR: TEXT query but asking for float - should fail!
# The query returns TEXT but return_tp expects float
name_as_float: float | None = pg_fetchvalue(
    None,
    query="SELECT name::TEXT FROM users LIMIT 1",
    return_tp=float,  # type: ignore[arg-type]  # WRONG! Should be str
)

# ERROR: INTEGER query but asking for str - should fail!
count_as_str: str | None = pg_fetchvalue(
    None,
    query="SELECT COUNT(*)::INTEGER FROM users",
    return_tp=str,  # type: ignore[arg-type]  # WRONG! Should be int
)

# ERROR: Assigning int result to str variable - should fail!
wrong_type: str | None = pg_fetchvalue(
    None,
    query="SELECT COUNT(*)::INTEGER FROM users",
    return_tp=int,  # type: ignore[assignment]  # Returns int, assigned to str!
)
