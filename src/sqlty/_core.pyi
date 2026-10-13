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
def schema_skipped_tables(schema_sql: str, *, dialect: str = "postgres") -> list[str]: ...
