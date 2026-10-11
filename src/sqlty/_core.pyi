"""Type stubs for the Rust extension module (sqlty._core).

These stubs ensure that mypy/pyright can type-check code that imports from
the Rust extension, even when the extension isn't compiled.
"""

def infer_type(
    sql: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
) -> str | None: ...
def rewrite_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    import_style: str | None = None,
    untyped: bool = False,
) -> str: ...
def strip_source(source: str, *, import_style: str | None = None) -> str: ...
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
) -> list[dict[str, str | int | None]]: ...
def skipped_strings(
    source: str,
    *,
    rules: dict[str, str] | None = None,
) -> list[dict[str, str | int | None]]: ...
