"""TypeCodec Protocol — pluggable type formatting for SQL analysis results.

The TypeCodec abstraction separates *what types are inferred* (SQLAnalyzer)
from *how they're formatted* (TypeCodec). This lets callers swap in custom
formatters without touching the analyzer.

Default: TupleCodec — formats as `tuple[int, str]`.
"""

from __future__ import annotations

from typing import Protocol


class TypeCodec(Protocol):
    """Protocol for formatting a list of Python type names into a type annotation string."""

    def format_type(self, column_types: list[str]) -> str:
        """Format column types into a single type annotation string.

        Args:
            column_types: List of Python type name strings (e.g. ["int", "str", "bool"])

        Returns:
            A type annotation string (e.g. "tuple[int, str, bool]")
        """
        ...


class TupleCodec:
    """Default codec — formats column types as a tuple type annotation.

    Examples:
        >>> codec = TupleCodec()
        >>> codec.format_type(["int"])
        'tuple[int]'
        >>> codec.format_type(["int", "str", "bool"])
        'tuple[int, str, bool]'
    """

    def format_type(self, column_types: list[str]) -> str:
        return f"tuple[{', '.join(column_types)}]"
