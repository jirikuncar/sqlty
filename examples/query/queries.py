"""Query registry using @sqlty decorator."""

from typing import Any

from sqlty import sqlty


@sqlty
def sql(query: str) -> Any:
    """Execute SQL queries."""
    return query
