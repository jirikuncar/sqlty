"""Type annotation checker — validates SQL[T](...) annotations against queries.

Checks that annotated types (tuple, TypedDict, NamedTuple, dataclass) match
the columns and types inferred from the SQL query. Emits diagnostics using
kebab-case rule names with ty-style severity.
"""

from __future__ import annotations

from sqlty._lint import Diagnostic, RuleConfig
from sqlty._sql_analyzer import ColumnInfo, SQLAnalyzer
from sqlty._type_extractor import field_accepts


class Checker:
    """Checks SQL queries against type annotations.

    Args:
        analyzer: SQLAnalyzer for type inference
        rule_config: Rule severity configuration
    """

    def __init__(
        self,
        analyzer: SQLAnalyzer,
        rule_config: RuleConfig | None = None,
    ) -> None:
        self._analyzer = analyzer
        self._rules = rule_config or RuleConfig()

    def check_column_count(
        self,
        annotation_fields: list[tuple[str, str]],
        columns: list[ColumnInfo],
        type_name: str = "tuple",
    ) -> Diagnostic | None:
        """Check column-count-mismatch rule.

        Args:
            annotation_fields: List of (name, type) pairs from the annotation
            columns: List of ColumnInfo from SQL analysis
            type_name: Name of the annotated type (for error message)
        """
        if not self._rules.is_enabled("column-count-mismatch"):
            return None

        if len(annotation_fields) != len(columns):
            return Diagnostic(
                rule="column-count-mismatch",
                message=f"{type_name} has {len(annotation_fields)} fields but query returns {len(columns)} columns",
                severity=self._rules.severity("column-count-mismatch"),
            )
        return None

    def check_column_types(
        self,
        annotation_fields: list[tuple[str, str]],
        columns: list[ColumnInfo],
        type_name: str = "tuple",
    ) -> list[Diagnostic]:
        """Check column-type-mismatch rule.

        Args:
            annotation_fields: List of (name, type) pairs from the annotation
            columns: List of ColumnInfo from SQL analysis
            type_name: Name of the annotated type
        """
        if not self._rules.is_enabled("column-type-mismatch"):
            return []

        diagnostics: list[Diagnostic] = []
        for (field_name, field_type), col in zip(annotation_fields, columns, strict=False):
            if not field_accepts(field_type, col.python_type):
                diagnostics.append(
                    Diagnostic(
                        rule="column-type-mismatch",
                        message=f"{type_name}.{field_name} is {field_type} but query column '{col.name}' infers {col.python_type}",
                        severity=self._rules.severity("column-type-mismatch"),
                    )
                )
        return diagnostics

    def check_column_names(
        self,
        annotation_fields: list[tuple[str, str]],
        columns: list[ColumnInfo],
        type_name: str = "tuple",
    ) -> list[Diagnostic]:
        """Check column-name-mismatch rule.

        Only applies to named types (TypedDict, NamedTuple, dataclass).
        Tuple annotations skip this check.

        Args:
            annotation_fields: List of (name, type) pairs from the annotation
            columns: List of ColumnInfo from SQL analysis
            type_name: Name of the annotated type
        """
        if not self._rules.is_enabled("column-name-mismatch"):
            return []

        diagnostics: list[Diagnostic] = []
        for (field_name, _), col in zip(annotation_fields, columns, strict=False):
            if field_name != col.name:
                diagnostics.append(
                    Diagnostic(
                        rule="column-name-mismatch",
                        message=f"{type_name}.{field_name} doesn't match query column '{col.name}'",
                        severity=self._rules.severity("column-name-mismatch"),
                    )
                )
        return diagnostics

    def check_select_star(
        self,
        sql: str,
        has_schema: bool = False,
    ) -> Diagnostic | None:
        """Check select-star and select-star-no-schema rules.

        Args:
            sql: The SQL query string
            has_schema: Whether a schema is available for expansion
        """
        # Simple check: does the SQL contain SELECT *
        if "SELECT *" not in sql.upper() and "SELECT\n*" not in sql.upper():
            return None

        if not has_schema:
            if self._rules.is_enabled("select-star-no-schema"):
                return Diagnostic(
                    rule="select-star-no-schema",
                    message="SELECT * used but no schema available",
                    severity=self._rules.severity("select-star-no-schema"),
                )
        else:
            if self._rules.is_enabled("select-star"):
                return Diagnostic(
                    rule="select-star",
                    message="SELECT * used — expand to explicit column list",
                    severity=self._rules.severity("select-star"),
                )

        return None

    def check_query(
        self,
        sql: str,
        annotation_fields: list[tuple[str, str]] | None = None,
        type_name: str = "tuple",
        has_schema: bool = False,
    ) -> list[Diagnostic]:
        """Run all applicable checks on a query.

        Args:
            sql: The SQL query string
            annotation_fields: Optional type annotation fields to check against
            type_name: Name of the annotated type
            has_schema: Whether a schema is available

        Returns:
            List of diagnostics (may be empty if all checks pass)
        """
        diagnostics: list[Diagnostic] = []

        # Check SELECT *
        star_diag = self.check_select_star(sql, has_schema=has_schema)
        if star_diag:
            diagnostics.append(star_diag)

        # If annotation provided, check against inferred columns
        if annotation_fields is not None:
            columns = self._analyzer.analyze_select(sql)
            if columns:
                # Check count
                count_diag = self.check_column_count(annotation_fields, columns, type_name)
                if count_diag:
                    diagnostics.append(count_diag)
                else:
                    # Only check types/names if counts match
                    diagnostics.extend(
                        self.check_column_types(annotation_fields, columns, type_name)
                    )
                    diagnostics.extend(
                        self.check_column_names(annotation_fields, columns, type_name)
                    )

        return diagnostics
