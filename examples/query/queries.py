"""Query registry for the example application."""

from sqlty import SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry.

    This class serves as a registry for all SQL queries in the application.
    Type-safe overloads are defined in the companion .pyi stub file.
    """

    pass


# Export as the main sql interface
sql = Query.sql
