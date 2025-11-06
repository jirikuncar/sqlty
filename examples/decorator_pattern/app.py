"""Example demonstrating the @sqlty decorator pattern.

This example shows how to use the @sqlty decorator to mark functions
for SQL type inference and stub generation.
"""

from typing import Any

from sqlty import SQL, sqlty


@sqlty
def sql(query: str) -> Any:
    """Simple SQL function decorated with @sqlty."""
    return query


@sqlty(schema="schema.sql")
def sql_with_schema(query: str) -> Any:
    """SQL function with schema path for enhanced type inference."""
    return query


q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
q2: SQL[tuple[int, str]] = sql_with_schema("SELECT id::INTEGER, name::TEXT FROM users")

# The functions have __sqlty__ and __schema_path__ attributes
assert hasattr(sql, "__sqlty__") and sql.__sqlty__ is True
assert hasattr(sql_with_schema, "__sqlty__") and sql_with_schema.__sqlty__ is True
assert hasattr(sql_with_schema, "__schema_path__")
assert sql_with_schema.__schema_path__ == "schema.sql"

print("✅ Decorator pattern working correctly!")
print(f"sql function has __sqlty__: {sql.__sqlty__}")
print(f"sql_with_schema function has __schema_path__: {sql_with_schema.__schema_path__}")
print(f"\nExecuted queries:")
print(f"  q = {q}")
print(f"  q2 = {q2}")
