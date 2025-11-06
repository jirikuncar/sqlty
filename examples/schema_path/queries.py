"""Query registries using @sqlty decorator with different schemas."""

from typing import Any

from sqlty import sqlty


@sqlty(schema="schema_a.sql")
def sql_a(query: str) -> Any:
    """Execute SQL queries against schema A."""
    return query


@sqlty(schema="schema_b.sql")
def sql_b(query: str) -> Any:
    """Execute SQL queries against schema B."""
    return query
