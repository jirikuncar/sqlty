"""Type stubs for the Rust extension module (sqlty._core)."""

import ast
from pathlib import Path
from typing import Any, final

from sqlty._lint import Diagnostic

@final
class Schema:
    @property
    def dialect(self) -> str: ...
    @property
    def absent(self) -> bool: ...

def load_schema(
    schema_sql: str,
    *,
    dialect: str = "postgres",
    types: list[tuple[str, str]] | None = None,
    name: str | None = None,
    absent: bool = False,
    databases: list[tuple[str, Schema | None]] | None = None,
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
) -> list[tuple[str, str, int | None, int | None, str | None, str | None]]: ...
def name_rules() -> list[str]: ...
def dialect_names() -> list[str]: ...
def python_types() -> list[str]: ...
def parse_error(sql: str, *, dialect: str = "postgres") -> str | None: ...
def checks_names(dialect: str) -> bool: ...
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
def add_param_types(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
) -> str: ...
def strip_source(source: str, *, import_style: str | None = None) -> str: ...
def format_source(source: str, *, dialect: str = ..., import_style: str | None = None) -> str: ...
def sqruff_findings(
    source: str, *, dialect: str = ..., import_style: str | None = None
) -> list[tuple[str, str, int, int, int]]: ...
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
def check_queries(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    rules: dict[str, str] | None = None,
    query_format: str = "auto",
    name: str = "",
) -> list[Diagnostic]: ...
def named_queries(
    source: str, *, query_format: str = "auto", name: str = ""
) -> list[tuple[str, str, list[tuple[str, str]]]]: ...
def module_facts(
    source: str,
) -> (
    tuple[
        list[str],
        list[str],
        list[tuple[str, str, str | None, int]],
        list[tuple[int, str]],
        list[tuple[str, str]],
        list[
            tuple[
                int,
                str,
                list[tuple[int | str, str | None]] | None,
                str,
                str | None,
                tuple[int, int],
            ]
        ],
    ]
    | None
): ...
def template_strings(
    source: str,
) -> list[tuple[int, tuple[int, int], str | None, list[str | None]]] | None: ...
def module_outline(source: str) -> tuple[ast.Module, list[ast.ClassDef]] | None: ...
def helper_sites(
    source: str, names: list[str]
) -> (
    list[
        tuple[
            tuple[int, int],
            str | None,
            str,
            list[_SiteArg],
            list[tuple[str | None, _SiteArg]],
            str | None,
        ]
    ]
    | None
): ...

_SiteArg = tuple[
    bool, tuple[int, int], str | None, str | None, list[tuple[int | str, str | None]] | None
]

def unparse_expression(source: str) -> str | None: ...
def type_definitions(
    source: str, module: str | None, version: tuple[int, int]
) -> list[str] | None: ...
def type_relations(
    source: str, version: tuple[int, int], columns: list[str | None]
) -> list[str] | None: ...
def row_diagnostics(
    source: str,
    *,
    version: tuple[int, int],
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
    module: str | None = None,
    coerce: list[str] = ...,
) -> (
    tuple[
        list[Diagnostic],
        list[tuple[str | None, str, str] | None],
        list[tuple[str | None, str, str]],
    ]
    | None
): ...
def module_diagnostics(
    source: str,
    *,
    version: tuple[int, int],
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    rules: dict[str, str] | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    module: str | None = None,
    coerce: list[str] = ...,
    index: ModuleIndex | None = None,
    name: str | None = None,
    variants: int | None = None,
) -> tuple[
    list[Diagnostic],
    list[tuple[str | None, str, str] | None],
    list[tuple[str | None, str, str]],
]: ...

type _Helper = tuple[
    list[str],
    list[str],
    str,
    str | None,
    str,
    str | None,
    str | None,
    str | None,
    bool,
    str | None,
]

@final
class ModuleIndex:
    def extended(
        self, steps: list[tuple[str, str, str, str | None, list[Path], str | None]]
    ) -> ModuleIndex: ...
    def helpers(self) -> dict[str, dict[str, _Helper]]: ...

def module_index(
    version: tuple[int, int],
    steps: list[tuple[str, str, str, str | None, list[Path], str | None]],
) -> ModuleIndex: ...
def module_helpers(source: str, version: tuple[int, int]) -> dict[str, _Helper]: ...
def call_variants(
    source: str, version: tuple[int, int], limit: int
) -> list[tuple[int, bool, list[tuple[str, list[int], bool]] | None, bool, str | None]] | None: ...
def module_calls(
    source: str,
    *,
    version: tuple[int, int],
    index: ModuleIndex | None = None,
    name: str | None = None,
    module: str | None = None,
    variants: int | None = None,
) -> list[tuple[tuple[int, int] | None, list[str], str | None, int | None]]: ...
def add_json_types(
    source: str,
    *,
    version: tuple[int, int],
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
) -> str: ...
def module_ignores(
    source: str, version: tuple[int, int]
) -> list[tuple[int, list[str] | None, int, int]] | None: ...
def rendered_compositions(
    source: str, version: tuple[int, int]
) -> list[tuple[int, bool, str | None]] | None: ...
def wrapped_queries(
    source: str, *, import_style: str | None = None
) -> list[tuple[int, str, str | None, tuple[int, int]]]: ...
def query_sites(
    source: str, *, import_style: str | None = None, any_calls: list[int] = ...
) -> list[tuple[int, str, str]]: ...

@final
class QueryAt:
    @property
    def text(self) -> str: ...
    @property
    def offset(self) -> int: ...
    @property
    def route(self) -> str: ...
    @property
    def span(self) -> tuple[int, int]: ...
    def source_offset(self, offset: int) -> int | None: ...

def query_at(
    source: str, offset: int, *, import_style: str | None = None, helper_starts: list[int] = ...
) -> QueryAt | None: ...
def complete_source(
    source: str,
    offset: int,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
    helper_starts: list[int] = ...,
    limit: int = 200,
) -> tuple[int, int, bool, list[tuple[str, str, str | None, str | None, bool | None]]] | None: ...
def hover_source(
    source: str,
    offset: int,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
    helper_starts: list[int] = ...,
) -> tuple[int, int, str] | None: ...
def highlight_source(
    source: str,
    *,
    dialect: str = "postgres",
    import_style: str | None = None,
    helper_starts: list[int] = ...,
) -> list[tuple[int, int, str]]: ...
def skills() -> list[tuple[str, str]]: ...
def serve() -> None:
    """Serve the language server on standard input and output until the client exits."""

def schema_skipped_tables(schema_sql: str | Schema, *, dialect: str = "postgres") -> list[str]: ...
def schema_skipped_objects(
    schema_sql: str | Schema, *, dialect: str = "postgres"
) -> list[tuple[str, str]]: ...

class ConfigSyntaxError(ValueError): ...

CONFIG_FILES: tuple[str, str, str]
TOOL_KEYS: tuple[str, ...]
DISCOVERY_KEYS: tuple[str, ...]
FUNCTION_KEYS: tuple[str, ...]

def load_config(
    data: bytes, file_name: str, source: str, *, required: bool
) -> tuple[dict[str, Any], list[str]] | None: ...
def validate_config(table: dict[str, Any], source: str) -> tuple[dict[str, Any], list[str]]: ...
def driver_types(driver: str, dialect: str) -> list[tuple[str, str]]: ...
