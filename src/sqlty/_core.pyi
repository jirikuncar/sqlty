"""Type stubs for the Rust extension module (sqlty._core)."""

from typing import final

from sqlty._lint import Diagnostic

@final
class Schema:
    @property
    def dialect(self) -> str: ...

def load_schema(
    schema_sql: str, *, dialect: str = "postgres", types: list[tuple[str, str]] | None = None
) -> Schema: ...
def infer_type(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> str | None: ...
def analyze_select(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> list[tuple[str, str | None, str | None]] | None: ...
def explain_select(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> list[tuple[str, tuple[str, str, str] | None, tuple[str, str, str] | None]] | None: ...
def analyze_columns(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> list[tuple[str, str | None, str | None, bool | None, str | None]] | None: ...
def analyze_params(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> list[tuple[int, str | None, str | None, str | None, bool | None, bool, str | None]] | None: ...
def composite_fields(
    type_name: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> list[tuple[str, str | None, str | None, bool | None]] | None: ...
def query_diagnostics(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> list[tuple[str, str]]: ...
def name_rules() -> list[str]: ...
def dialect_names() -> list[str]: ...
def python_types() -> list[str]: ...
def parse_error(sql: str, *, dialect: str = "postgres") -> str | None: ...
def returns_rows(sql: str, *, dialect: str = "postgres") -> bool: ...
def rewrite_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    keep_lines: bool = False,
) -> str: ...
def strip_source(source: str, *, import_style: str | None = None) -> str: ...
def fix_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    rules: dict[str, str] | None = None,
    suppressed: dict[int, list[str] | None] | None = None,
) -> str: ...
def expand_star(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
) -> str | None: ...
def select_star_finding(
    sql: str, *, dialect: str = "postgres", has_schema: bool = False
) -> tuple[str, str] | None: ...
def check_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    any_calls: list[int] = ...,
) -> list[Diagnostic]: ...
def wrapped_queries(
    source: str, *, import_style: str | None = None
) -> list[tuple[int, str, str | None, tuple[int, int]]]: ...
def query_sites(
    source: str, *, import_style: str | None = None, any_calls: list[int] = ...
) -> list[tuple[int, str, str]]: ...
def skills() -> list[tuple[str, str]]: ...
def serve() -> None:
    """Serve the language server on standard input and output until the client exits."""

def schema_skipped_tables(schema_sql: str | Schema, *, dialect: str = "postgres") -> list[str]: ...
def schema_skipped_objects(
    schema_sql: str | Schema, *, dialect: str = "postgres"
) -> list[tuple[str, str]]: ...
