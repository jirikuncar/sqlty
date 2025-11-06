"""Example demonstrating the @sqlty decorator pattern."""

from typing import Any

from sqlty import SQL, sqlty


@sqlty
def sql(query: str) -> Any:
    """Simple SQL function decorated with @sqlty."""
    return query


@sqlty(schema="schema.sql")
def sql_with_schema(query: str) -> Any:
    """SQL function with schema path for schema based type inference."""
    return query


q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
q2: SQL[tuple[int, str]] = sql_with_schema("SELECT id::INTEGER, name::TEXT FROM users")
