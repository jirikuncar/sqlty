from typing import Any

from sqlty import sqlty


@sqlty
def user_sql(query: str) -> Any:
    """User-related queries."""
    return query


@sqlty
def post_sql(query: str) -> Any:
    """Post-related queries."""
    return query


@sqlty
def comment_sql(query: str) -> Any:
    """Comment-related queries."""
    return query
