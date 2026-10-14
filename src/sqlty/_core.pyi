"""Type stubs for the Rust extension module (sqlty._core)."""

def infer_type(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
) -> str | None: ...
def analyze_select(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
) -> list[tuple[str, str | None, str | None]] | None: ...
def query_diagnostics(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
) -> list[tuple[str, str]]: ...
def name_rules() -> list[str]: ...
def parse_error(sql: str, *, dialect: str = "postgres") -> str | None: ...
def returns_rows(sql: str, *, dialect: str = "postgres") -> bool: ...
def rewrite_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    keep_lines: bool = False,
) -> str: ...
def strip_source(source: str, *, import_style: str | None = None) -> str: ...
def fix_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    rules: dict[str, str] | None = None,
    suppressed: dict[int, list[str] | None] | None = None,
) -> str: ...
def expand_star(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
) -> str | None: ...
def check_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
    untyped: bool = False,
) -> list[dict[str, str | int | None]]: ...
def wrapped_queries(
    source: str, *, import_style: str | None = None
) -> list[tuple[int, str, str | None]]: ...
def query_sites(source: str, *, import_style: str | None = None) -> list[tuple[int, str, str]]: ...
def schema_skipped_tables(schema_sql: str, *, dialect: str = "postgres") -> list[str]: ...
def schema_skipped_objects(
    schema_sql: str, *, dialect: str = "postgres"
) -> list[tuple[str, str]]: ...
