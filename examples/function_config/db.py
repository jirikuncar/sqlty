"""Database utility functions for demonstration."""

from typing import Any


def pg_fetchvalue[T](
    conn: Any,
    query: str,
    return_tp: type[T] | None = None,
    /,
    **params: Any,
) -> T | None:
    """Fetch a single value from the database.

    This is a simplified example of a database helper function.
    In a real application, this would execute the query against
    a PostgreSQL database and return the result.

    Args:
        conn: Database connection
        query: SQL query string
        return_tp: Expected return type
        **params: Query parameters

    Returns:
        Single value from the query result
    """
    # In a real implementation, this would execute the query
    # For example purposes, we just return None
    return None


def pg_fetchall[T](
    conn: Any,
    query: str,
    return_tp: type[T] | None = None,
    /,
    **params: Any,
) -> list[T]:
    """Fetch all values from the database.

    This is a simplified example of a database helper function.
    In a real application, this would execute the query against
    a PostgreSQL database and return the results.

    Args:
        conn: Database connection
        query: SQL query string
        return_tp: Expected return type (for each row)
        **params: Query parameters

    Returns:
        List of values from the query result
    """
    # In a real implementation, this would execute the query
    # For example purposes, we just return an empty list
    return []


def execute_query(query: str, **params: Any) -> list[Any]:
    """Execute a query and return all results.

    This is a simplified example that demonstrates type inference
    without an explicit return_type_param.

    Args:
        query: SQL query string
        **params: Query parameters

    Returns:
        List of query results
    """
    # In a real implementation, this would execute the query
    # For example purposes, we just return an empty list
    return []
