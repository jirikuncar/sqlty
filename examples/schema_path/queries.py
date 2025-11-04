"""Query registry for the example application."""

from sqlty import SQLRegistry


class QueryA(SQLRegistry):
    """Application-specific SQL query registry.

    This class serves as a registry for all SQL queries in the application.
    Type-safe overloads are defined in the companion .pyi stub file.
    """

    __schema_path__ = "schema_a.sql"


class QueryB(SQLRegistry):
    """Application-specific SQL query registry.

    This class serves as a registry for all SQL queries in the application.
    Type-safe overloads are defined in the companion .pyi stub file.
    """

    __schema_path__ = "schema_b.sql"
