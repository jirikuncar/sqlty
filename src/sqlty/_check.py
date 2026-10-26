"""The Python checks of ``check_source``: row types (TypedDict, NamedTuple, dataclass,
Pydantic, tuples) and helper call parameters against the query columns.

``check_source`` in ``sqlty.core`` runs the engine's checks, then these passes over one
``_Check`` context, then the inline suppressions.
"""

from __future__ import annotations

import ast
import re
import typing
from collections.abc import Iterable, Mapping
from typing import TYPE_CHECKING, NamedTuple, cast

from sqlty._core import analyze_columns as _analyze_columns
from sqlty._core import check_source as _check_source
from sqlty._core import composite_fields as _composite_fields
from sqlty._core import parse_error as _parse_error
from sqlty._core import query_diagnostics as _query_diagnostics
from sqlty._core import select_star_finding as _select_star_finding
from sqlty._core import wrapped_queries as _wrapped_queries
from sqlty._drivers import MARKER, find_driver_calls, find_template_calls
from sqlty._helpers import HelperIndex, find_helper_calls
from sqlty._lint import NAME_RULES, Diagnostic
from sqlty._params import mismatch
from sqlty._source import ParsedSource, is_any

if TYPE_CHECKING:
    from collections.abc import Callable

    from sqlty._core import Schema
    from sqlty._helpers import HelperCall
    from sqlty._lint import RuleConfig
    from sqlty._type_extractor import TypeInfo
    from sqlty.core import ParamInfo

# The `--sql` marker that opens a query string, as the engine reads it.


def is_any_row(call: HelperCall) -> bool:
    """Whether the row type is ``typing.Any``: an explicit opt-out of checks."""
    return call.row is not None and is_any(call.row, call.typing_aliases)


_CONTAINERS = (dict, list, set, frozenset, tuple)


class _RowColumn(NamedTuple):
    """A result column with whether it can be NULL (``None`` when unknown), for row type checks."""

    name: str
    python_type: str | None
    sql_type: str | None
    nullable: bool | None
    json_shape: str | None = None
    """The shape of the JSON the column builds, as the engine spells it (see ``_json_shape``)."""


class RowUsage:
    """Which fields of the row types a run checks some query needs ``| None`` for.

    ``redundant-optional`` is a question about a row type, not a query: a field is
    redundant only when no query that loads the type can return NULL for it.
    """

    def __init__(self) -> None:
        self.needed: set[tuple[str | None, str, str]] = set()
        self.redundant: list[tuple[tuple[str | None, str, str], Diagnostic]] = []

    def filter(self, diagnostics: list[Diagnostic]) -> list[Diagnostic]:
        """``diagnostics`` without the ``redundant-optional`` reports another query refutes."""
        refuted = {id(diag) for key, diag in self.redundant if key in self.needed}
        return [diag for diag in diagnostics if id(diag) not in refuted]


class _Check:
    """Inputs, derived call sites and the ``report`` sink shared by the check passes."""

    def __init__(
        self,
        parsed: ParsedSource,
        diagnostics: list[Diagnostic],
        *,
        dialect: str,
        schema_sql: str | Schema | None,
        rule_config: RuleConfig,
        found: list[HelperCall],
        driver_calls: list[HelperCall],
        queries: list[tuple[int, str, str | None]],
        type_modules: Iterable[str] | None,
        helpers: HelperIndex | None,
        module: str | None,
    ) -> None:
        from sqlty._type_extractor import extract_type_definitions

        self.parsed = parsed
        self.diagnostics = diagnostics
        self.dialect = dialect
        self.schema_sql = schema_sql
        self.rule_config = rule_config
        self.found = found
        self.driver_calls = driver_calls
        self.calls = [call for call in found if call.query is not None]
        self._unparsed: dict[str, str] = {}
        self.rows = [self._unparse(call.row) if call.row else None for call in self.calls]
        named = [
            (call.line, row, _bound(call, dialect))
            for call, row in zip(self.calls, self.rows, strict=True)
            if row
        ]
        self.type_defs: dict[str, TypeInfo] = (
            extract_type_definitions(parsed, helpers, module, type_modules)
            if queries or named
            else {}
        )
        # The row types of the module an imported row type comes from, by local name.
        self.imported_defs: dict[str, dict[str, TypeInfo]] = {}
        self.row_usage = RowUsage()
        if helpers is not None:
            extra = frozenset(type_modules or ())
            for name in {q[1] for q in [*queries, *named]} - self.type_defs.keys():
                if (imported := helpers.row_type(parsed, name, extra)) is not None:
                    self.type_defs[name], self.imported_defs[name] = imported
        # A row type named through an alias (`type Row = User`) is checked as what it names.
        self.aliased: list[tuple[int, str, str | None, ast.expr]] = []
        resolved = []
        for line, name, query in [*queries, *named]:
            target = _alias_target(name, parsed.type_aliases)
            if target is None:
                if name not in self.type_defs:
                    self._specialize(name)
                resolved.append((line, name, query))
            elif (base := ast.unparse(target)) in self.type_defs:
                resolved.append((line, base, query))
            else:
                self.aliased.append((line, name, query, target))
        self.user_queries = [q for q in resolved if q[1] in self.type_defs]

    def _specialize(self, name: str) -> None:
        """Add ``Row[int]`` to the row types when ``Row`` is a generic class of this source."""
        from sqlty._type_extractor import specialize

        try:
            row = ast.parse(name, mode="eval").body
        except SyntaxError:
            return
        if not (isinstance(row, ast.Subscript) and isinstance(row.value, ast.Name)):
            return
        info = self.type_defs.get(row.value.id)
        node = next(
            (c for c in self.parsed.classes if info is not None and c.lineno == info.lineno),
            None,
        )
        if info is None or node is None:
            return
        args = list(row.slice.elts) if isinstance(row.slice, ast.Tuple) else [row.slice]
        if (generic := specialize(info, node, args, self.parsed.typing_aliases)) is not None:
            self.type_defs[name] = generic

    def composite(self, sql_type: str) -> list[_RowColumn] | None:
        """The fields of the composite type ``sql_type`` as columns; None when it is not one."""
        fields = _composite_fields(sql_type, dialect=self.dialect, schema_sql=self.schema_sql)
        return None if fields is None else [_RowColumn(*field) for field in fields]

    def _unparse(self, row: ast.expr) -> str:
        """``ast.unparse(row)``, once per distinct source span in the file."""
        if (key := self.parsed.segment(row)) is None:
            return ast.unparse(row)
        if key not in self._unparsed:
            self._unparsed[key] = ast.unparse(row)
        return self._unparsed[key]

    def report(self, rule: str, message: str, line: int) -> Diagnostic | None:
        if not self.rule_config.is_enabled(rule):
            return None
        diagnostic: Diagnostic = {
            "rule": rule,
            "message": message,
            "severity": self.rule_config.severity(rule).value,
            "line": line,
        }
        self.diagnostics.append(diagnostic)
        return diagnostic


def _check_unchecked_queries(ctx: _Check) -> None:
    for call in ctx.found:
        if call.query is None and not is_any_row(call):
            if call.dynamic:
                ctx.report(
                    "dynamic-query",
                    "query is not a string literal, so it is not checked",
                    call.line,
                )
            else:
                ctx.report(
                    "unchecked-query",
                    "psycopg query is not built from literals only, so it is not checked",
                    call.line,
                )


def _check_params(ctx: _Check) -> None:
    if not (
        ctx.rule_config.is_enabled("param-type-mismatch")
        or ctx.rule_config.is_enabled("param-count-mismatch")
    ):
        return
    singles = {(call.line, call.single, call.driver) for call in ctx.driver_calls if call.single}
    if ctx.rule_config.is_enabled("param-type-mismatch"):
        for line, single, driver in sorted(singles):
            ctx.report(
                "param-type-mismatch",
                f"parameters are a single {single}, which {driver} rejects: pass a sequence, `(value,)`",
                line,
            )
    for call in [*ctx.calls, *ctx.driver_calls]:
        if call.params is not None:
            for rule, message in _param_diagnostics(call, ctx.dialect, ctx.schema_sql):
                ctx.report(rule, message, call.line)


def _check_typeddict_inheritance(ctx: _Check) -> None:
    if not ctx.rule_config.is_enabled("typeddict-inheritance"):
        return
    names = [name for _, name, _ in ctx.user_queries if name not in ctx.imported_defs]
    for info in _typeddict_lineage(names, ctx.type_defs):
        for message in info.errors:
            ctx.report("typeddict-inheritance", message, info.lineno)


def _check_select_star(ctx: _Check) -> None:
    for call in ctx.calls:
        if call.query and (
            found := _select_star_finding(
                call.query, dialect=ctx.dialect, has_schema=ctx.schema_sql is not None
            )
        ):
            ctx.report(*found, call.line)


def _check_unparsed(ctx: _Check) -> None:
    if not ctx.rule_config.is_enabled("unparsed-query"):
        return
    for call in ctx.calls:
        if (
            call.query is not None
            and not MARKER.match(call.query)
            and (error := _parse_error(call.query, dialect=ctx.dialect))
        ):
            message = f"query does not parse: {error}"
            if "found: {" in error and (field := _FORMAT_FIELD.search(call.query)):
                message += f"; `{field[0]}` is a `.format()` field: fill it with `sql.SQL(...).format(...)`, or pass a value as `%s`"
            ctx.report("unparsed-query", message, call.line)


_FORMAT_FIELD = re.compile(r"\{\w*\}")


def _check_names(ctx: _Check) -> None:
    if not any(ctx.rule_config.is_enabled(name) for name in NAME_RULES):
        return
    for call in ctx.calls:
        if call.query is None or MARKER.match(call.query):
            continue
        for rule, message in _query_diagnostics(
            call.query, dialect=ctx.dialect, schema_sql=ctx.schema_sql
        ):
            ctx.report(rule, message, call.line)


def _check_structural(ctx: _Check) -> None:
    """Rows named by a type defined in this source (or an imported one)."""
    for line, name, query in ctx.user_queries:
        if (columns := _columns(query, ctx.dialect, ctx.schema_sql)) is None:
            continue
        info = ctx.type_defs[name]
        defs = ctx.imported_defs.get(name, ctx.type_defs)
        # A field this query can fill with NULL needs `| None`, whatever other queries do.
        pairs = (
            zip((field for field, _ in info.fields), columns, strict=False)
            if info.kind == "NamedTuple"
            else ((info.field_for(col.name), col) for col in columns)
        )
        for field, col in pairs:
            if field is not None and col.nullable is not False:
                ctx.row_usage.needed.add((info.module, info.name, field))
        for rule, message in _structural_diagnostics(
            name, info, columns, ctx.rule_config, defs, ctx.composite
        ):
            diagnostic = ctx.report(rule, message, line)
            if diagnostic is not None and rule == "redundant-optional":
                field = message.split(" is ", 1)[0].removeprefix(f"{name}.")
                ctx.row_usage.redundant.append(((info.module, info.name, field), diagnostic))


_PSYCOPG_PLACEHOLDER = re.compile(r"%%|%\((?P<name>[^)]+)\)s|%s")


def _bound(call: HelperCall, dialect: str) -> str | None:
    """The query of ``call`` with the placeholders it binds to non-``None`` values marked so.

    A bare placeholder reads as a value that may be NULL, since a caller may bind ``None``.
    Where the call passes an argument whose type excludes ``None`` (a literal, an
    annotated variable), the placeholder is respelled as one whose nullability is not
    judged, so the column or JSON member it fills is not reported as nullable.
    """
    if call.query is None or not call.params:
        return call.query
    non_null = {arg.key for arg in call.params if arg.type not in (None, "None")}
    if not non_null:
        return call.query
    marker = "$0" if dialect.lower() in ("postgres", "postgresql") else "?"
    position = 0

    def respell(match: re.Match[str]) -> str:
        nonlocal position
        if match[0] == "%%":
            return match[0]
        key: int | str
        if match["name"] is not None:
            key = match["name"]
        else:
            position += 1
            key = position
        return marker if key in non_null else match[0]

    return _PSYCOPG_PLACEHOLDER.sub(respell, call.query)


def _row_type_of(annotation: ast.expr | None, type_defs: Mapping[str, TypeInfo]) -> TypeInfo | None:
    """The row type in ``type_defs`` an annotation names, through ``X | None`` and ``Optional[X]``."""
    match annotation:
        case ast.BinOp(op=ast.BitOr(), left=left, right=ast.Constant(value=None)):
            return _row_type_of(left, type_defs)
        case ast.BinOp(op=ast.BitOr(), left=ast.Constant(value=None), right=right):
            return _row_type_of(right, type_defs)
        case ast.Subscript(value=value, slice=inner) if ast.unparse(value).endswith("Optional"):
            return _row_type_of(inner, type_defs)
        case ast.Name() | ast.Attribute():
            return type_defs.get(ast.unparse(annotation))
    return None


def _check_aliases(ctx: _Check) -> None:
    """Rows named by a module-level alias of a tuple or scalar type (``type Row = tuple[int, str]``)."""
    shapes: dict[str, _RowShape | None] = {}
    for line, name, query, target in ctx.aliased:
        if (columns := _columns(query, ctx.dialect, ctx.schema_sql)) is None:
            continue
        if name not in shapes:
            shapes[name] = _row_shape(target, name, ctx.parsed.typing_aliases)
        for rule, message in _value_diagnostics(
            shapes[name], columns, ctx.rule_config, ctx.type_defs
        ):
            ctx.report(rule, message, line)


def _alias_target(name: str, aliases: Mapping[str, ast.expr]) -> ast.expr | None:
    """What the alias ``name`` stands for, through aliases of aliases; ``None`` for no alias."""
    target = None
    for _ in range(16):
        if name not in aliases:
            break
        target = aliases[name]
        name = ast.unparse(target)
    return target


def _check_values(ctx: _Check) -> None:
    """Rows spelled as a tuple, ``list[...]`` etc. rather than a named type."""
    shapes: dict[str, _RowShape | None] = {}
    for call, row in zip(ctx.calls, ctx.rows, strict=True):
        if call.row is None or row is None or row in ctx.type_defs:
            continue
        if _alias_target(row, ctx.parsed.type_aliases) is not None:
            continue
        if (columns := _columns(_bound(call, ctx.dialect), ctx.dialect, ctx.schema_sql)) is None:
            continue
        if row not in shapes:
            shapes[row] = _row_shape(call.row, row, call.typing_aliases)
        for rule, message in _value_diagnostics(
            shapes[row], columns, ctx.rule_config, ctx.type_defs
        ):
            ctx.report(rule, message, call.line)


_CHECK_PASSES = (
    _check_unchecked_queries,
    _check_params,
    _check_typeddict_inheritance,
    _check_select_star,
    _check_unparsed,
    _check_names,
    _check_structural,
    _check_aliases,
    _check_values,
)


def diagnostics(
    parsed: ParsedSource,
    *,
    dialect: str,
    schema_sql: str | Schema | None,
    rules: dict[str, str] | None,
    import_style: str | None,
    untyped: bool,
    helpers: HelperIndex | None,
    module: str | None,
    type_modules: Iterable[str] | None = None,
    row_usage: RowUsage | None = None,
) -> list[Diagnostic]:
    """Every diagnostic before inline suppression: the Rust pass, then the Python passes."""
    from sqlty._lint import RuleConfig

    diagnostics: list[Diagnostic] = []
    if parsed.may_need_rust_check(import_style):
        diagnostics = cast(
            "list[Diagnostic]",
            _check_source(
                parsed.text,
                dialect=dialect,
                schema_sql=schema_sql,
                rules=rules,
                import_style=import_style,
                untyped=untyped,
                any_calls=parsed.any_calls(import_style),
            ),
        )
    found = find_helper_calls(parsed, helpers, module)
    typed = {line for call in found for line in call.typed_lines}
    diagnostics = [
        d for d in diagnostics if not (d["rule"] == "stale-rewrite" and d.get("line") in typed)
    ]
    driver_calls = find_driver_calls(parsed) + find_template_calls(parsed)
    queries = [
        (line, name, query)
        for line, name, query in (
            _wrapped_queries(parsed.text, import_style=import_style)
            if parsed.may_wrap(import_style)
            else []
        )
        if not name.startswith("tuple[")
    ]
    if not (queries or found or driver_calls):
        return diagnostics
    ctx = _Check(
        parsed,
        diagnostics,
        dialect=dialect,
        schema_sql=schema_sql,
        rule_config=RuleConfig(overrides=rules),
        found=found,
        driver_calls=driver_calls,
        queries=queries,
        type_modules=type_modules,
        helpers=helpers,
        module=module,
    )
    ctx.row_usage = row_usage or RowUsage()
    for check_pass in _CHECK_PASSES:
        check_pass(ctx)
    diagnostics.sort(key=lambda diagnostic: diagnostic["line"])
    return diagnostics


def _param_diagnostics(
    call: HelperCall, dialect: str, schema_sql: str | Schema | None
) -> list[tuple[str, str]]:
    """``(rule, message)`` for the arguments a call passes for its query's placeholders."""
    assert call.query is not None and call.params is not None
    from sqlty.core import analyze_params

    params = analyze_params(call.query, dialect=dialect, schema_sql=schema_sql)
    if params is None:
        return []
    named = bool(params) and all(p.name is not None for p in params)
    positional = all(p.name is None for p in params)
    passed = {arg.key: arg for arg in call.params}
    by_key: dict[int | str, ParamInfo] = {
        (p.name if named and p.name else p.number): p for p in params
    }
    by_name = bool(passed) and all(isinstance(key, str) for key in passed)
    if (named and passed and not by_name) or (positional and by_name):
        return []
    found: list[tuple[str, str]] = []
    if positional and len(passed) != len(by_key):
        found.append(
            (
                "param-count-mismatch",
                f"query has {len(by_key)} parameter{'' if len(by_key) == 1 else 's'}, "
                f"the call passes {len(passed)}",
            )
        )
    elif named and (missing := [f"'{key}'" for key in by_key if key not in passed]):
        found.append(
            (
                "param-count-mismatch",
                f"call is missing parameter{'' if len(missing) == 1 else 's'} {', '.join(missing)}",
            )
        )
    # sqlite3 is dynamically typed, and a cast converts whatever it is given.
    typed = dialect.lower() != "sqlite"
    postgres = dialect.lower() in ("postgres", "postgresql")
    for key, arg in passed.items():
        if (param := by_key.get(key)) is None:
            continue
        label = f"parameter {key}" if isinstance(key, int) else f"parameter '{key}'"
        if arg.type == "None" or (typed and not param.cast):
            expected = param.python_type
            if (
                expected is None
                and call.driver == "asyncpg"
                and param.untyped_sql_type == "INTERVAL"
            ):
                expected = "datetime.timedelta"
            if problem := mismatch(
                expected,
                param.nullable,
                arg.type,
                postgres=postgres,
                asyncpg=call.driver == "asyncpg",
            ):
                found.append(("param-type-mismatch", f"{label} {problem}"))
    if call.driver == "asyncpg":
        for param in params:
            if param.untyped_sql_type and param.sql_type:
                untyped, sql_type = param.untyped_sql_type.lower(), param.sql_type.lower()
                found.append(
                    (
                        "param-type-mismatch",
                        f"parameter {param.number} is an `{untyped}` to PostgreSQL, as asyncpg sends it "
                        f"untyped, so it cannot stand for a `{sql_type}`: "
                        f"cast it, `${param.number}::{sql_type}`",
                    )
                )
    return found


def _columns(
    query: str | None, dialect: str, schema_sql: str | Schema | None
) -> list[_RowColumn] | None:
    """Columns of a query and whether each can be NULL; ``None`` when it has none or cannot be analysed."""
    if query is None:
        return None
    try:
        columns = _analyze_columns(query, dialect=dialect, schema_sql=schema_sql)
    except RuntimeError:
        return None
    return [_RowColumn(*column) for column in columns] if columns else None


def _nullability_diagnostics(
    label: str,
    declared: typing.Any,
    col: _RowColumn,
    rules: RuleConfig,
    item: str = "query column",
) -> list[tuple[str, str]]:
    """``nullable-mismatch`` and ``redundant-optional`` for one field and its column.

    ``label`` reads ``<field>`` or ``<row> item <i>``, and is followed by ``is <annotation>``;
    ``declared`` is the resolved annotation. A column of unknown nullability, or an
    annotation that can't be resolved, is never reported.
    """
    from sqlty._type_extractor import admits_none, is_optional
    from sqlty._type_extractor.subtype import _type_repr

    if col.nullable is None or declared is None:
        return []
    if col.nullable and rules.is_enabled("nullable-mismatch") and admits_none(declared) is False:
        return [
            (
                "nullable-mismatch",
                f"{label} is {_type_repr(declared)} but {item} '{col.name}' can be NULL",
            )
        ]
    if not col.nullable and rules.is_enabled("redundant-optional") and is_optional(declared):
        return [
            (
                "redundant-optional",
                f"{label} is {_type_repr(declared)} but {item} '{col.name}' is never NULL",
            )
        ]
    return []


def _json_diagnostics(
    label: str,
    annotation: ast.expr | None,
    col: _RowColumn,
    type_defs: Mapping[str, TypeInfo],
    aliases: Mapping[str, str],
    rules: RuleConfig,
) -> list[tuple[str, str]]:
    """``json-shape-mismatch`` for one field and a JSON column of known shape.

    ``label`` reads ``<field>`` or ``<row> item <i>`` and is followed by ``is <annotation>``.
    An annotation that can't be read is never reported.
    """
    from sqlty._json_shape import json_problems, summarize

    if col.json_shape is None or annotation is None or not rules.is_enabled("json-shape-mismatch"):
        return []
    problems = json_problems(col.json_shape, annotation, type_defs, aliases, col.name)
    if not problems:
        return []
    return [
        (
            "json-shape-mismatch",
            f"{label} is {ast.unparse(annotation)} but query column '{col.name}' builds JSON "
            f"that does not fit: {summarize(problems)}",
        )
    ]


def _literal_diagnostics(
    label: str,
    declared: typing.Any,
    col: _RowColumn,
    rules: RuleConfig,
    item: str = "query column",
) -> list[tuple[str, str]]:
    """``literal-mismatch`` and ``redundant-literal`` for one field and its column.

    Reported only when the annotation is made of ``Literal`` members (and ``None``) and the
    column's values are known: ``str``, ``int``, ``Any`` or an enum take any value, so
    there is nothing to compare.
    """
    from sqlty._type_extractor import column_literal_values, declared_literal_values
    from sqlty._type_extractor.subtype import _type_repr

    if declared is None or not col.python_type:
        return []
    inferred = column_literal_values(col.python_type)
    if inferred is None:
        return []
    admitted = declared_literal_values(declared, inferred)
    if admitted is None:
        return []
    missing = [v for v in inferred if v not in admitted]
    if missing and rules.is_enabled("literal-mismatch"):
        return [
            (
                "literal-mismatch",
                f"{label} is {_type_repr(declared)} but {item} '{col.name}' "
                f"can be {', '.join(map(repr, missing))}",
            )
        ]
    extra = sorted((v for v in admitted if v not in inferred), key=repr)
    if not missing and extra and rules.is_enabled("redundant-literal"):
        return [
            (
                "redundant-literal",
                f"{label} is {_type_repr(declared)} but {item} '{col.name}' "
                f"is never {', '.join(map(repr, extra))}",
            )
        ]
    return []


def _field_diagnostics(
    label: str,
    declared: typing.Any,
    col: _RowColumn,
    rules: RuleConfig,
    item: str = "query column",
) -> list[tuple[str, str]]:
    """Nullability and value-set diagnostics for one field and its column (or composite field)."""
    return _nullability_diagnostics(label, declared, col, rules, item) + _literal_diagnostics(
        label, declared, col, rules, item
    )


class _RowShape(NamedTuple):
    """The row type of helper calls, resolved once per file for every call that spells it."""

    text: str
    is_tuple: bool
    targets: list[tuple[str, str, typing.Any, ast.expr]]
    accepts: dict[tuple[int, str], bool]
    aliases: Mapping[str, str]


def _row_shape(row: ast.expr, text: str, aliases: dict[str, str]) -> _RowShape | None:
    """What each column of a helper call's row type ``text`` must be; ``None`` when unchecked.

    A scalar type takes one column, ``tuple[A, B]`` one column per item. Other
    row types (an unknown class, ``dict``, ``tuple[int, ...]``) are not checked.
    """
    from sqlty._type_extractor.ast_fields import _ast_type

    if isinstance(row, ast.Subscript) and _ast_type(row.value, aliases) is tuple:
        items = list(row.slice.elts) if isinstance(row.slice, ast.Tuple) else [row.slice]
        if not items or any(isinstance(i, ast.Constant) and i.value is ... for i in items):
            return None
        annotations = [ast.unparse(item) for item in items]
        targets = [
            (f"{text} item {i} is {annotation}", annotation, _ast_type(item, aliases), item)
            for i, (item, annotation) in enumerate(zip(items, annotations, strict=True))
        ]
        return _RowShape(text, True, targets, {}, aliases)
    resolved = _ast_type(row, aliases)
    if resolved is None or resolved in _CONTAINERS or typing.get_args(resolved):
        return None
    return _RowShape(text, False, [(f"{text} is expected", text, resolved, row)], {}, aliases)


def _value_diagnostics(
    shape: _RowShape | None,
    columns: list[_RowColumn],
    rules: RuleConfig,
    type_defs: Mapping[str, TypeInfo],
) -> list[tuple[str, str]]:
    """Compare the row type of a helper call with the query columns."""
    from sqlty._type_extractor.compat import _accepts

    if shape is None:
        return []
    if len(shape.targets) != len(columns):
        takes = f"has {len(shape.targets)} items" if shape.is_tuple else "takes 1 column"
        return [
            (
                "column-count-mismatch",
                f"{shape.text} {takes} but query returns {len(columns)} columns",
            )
        ]
    found = []
    for i, ((label, annotation, resolved, node), col) in enumerate(
        zip(shape.targets, columns, strict=True)
    ):
        item = f"{shape.text} item {i}" if shape.is_tuple else "row type"
        found.extend(_json_diagnostics(item, node, col, type_defs, shape.aliases, rules))
        if resolved is None or not col.python_type:
            continue
        key = (i, col.python_type)
        if key not in shape.accepts:
            shape.accepts[key] = _accepts(resolved, annotation, col.python_type)
        if not shape.accepts[key]:
            found.append(
                (
                    "column-type-mismatch",
                    f"{label} but query column '{col.name}' infers {col.python_type}",
                )
            )
        else:
            found.extend(_field_diagnostics(item, resolved, col, rules))
    return found


def apply_suppressions(
    parsed: ParsedSource,
    diagnostics: list[Diagnostic],
    rules: dict[str, str] | None,
) -> list[Diagnostic]:
    """Drop diagnostics suppressed by a comment on their line; add ``unused-ignore``."""
    from sqlty._lint import RuleConfig

    ignores = parsed.ignores
    if not ignores:
        return diagnostics

    kept: list[Diagnostic] = []
    used: set[tuple[int, str | None]] = set()
    for diag in diagnostics:
        ignore = ignores.get(diag["line"])
        if ignore is None or (ignore.rules is not None and diag["rule"] not in ignore.rules):
            kept.append(diag)
        else:
            used.add((ignore.line, None if ignore.rules is None else diag["rule"]))

    rule_config = RuleConfig(overrides=rules)
    if rule_config.is_enabled("unused-ignore"):
        severity = rule_config.severity("unused-ignore").value
        for ignore in ignores.values():
            unused: list[str | None]
            if ignore.rules is None:
                unused = [None] if (ignore.line, None) not in used else []
            else:
                unused = [name for name in ignore.rules if (ignore.line, name) not in used]
            for name in unused:
                written = "# sqlty: ignore" if name is None else f"# sqlty: ignore[{name}]"
                kept.append(
                    {
                        "rule": "unused-ignore",
                        "message": f"'{written}' suppresses no diagnostic",
                        "severity": severity,
                        "line": ignore.line,
                    }
                )
    return kept


def _typeddict_lineage(names: list[str], type_defs: dict[str, TypeInfo]) -> list[TypeInfo]:
    """``names`` and their same-source TypedDict bases, bases first, each once."""
    seen: dict[str, TypeInfo] = {}

    def visit(name: str) -> None:
        if name in seen or (info := type_defs.get(name)) is None:
            return
        for base in info.bases:
            visit(base)
        seen[name] = info

    for name in names:
        visit(name)
    return list(seen.values())


def _structural_diagnostics(
    type_name: str,
    info: TypeInfo,
    columns: list[_RowColumn],
    rules: RuleConfig,
    type_defs: Mapping[str, TypeInfo],
    composite: Callable[[str], list[_RowColumn] | None] | None = None,
    source: str = "query",
) -> list[tuple[str, str]]:
    """Compare a user type with query columns, returning ``(rule, message)`` pairs.

    TypedDicts, dataclasses and Pydantic models match columns to fields by
    name, as ``dict_row`` and ``class_row`` pass them: fields with a default
    (``NotRequired`` keys) may be absent, and a column without a field is a
    violation, unless a Pydantic model allows it (``extra="allow"``) or
    ``extra_items`` admits it (type-checked against it). Key-set violations are
    ``typeddict-key-mismatch``. NamedTuples match by position.
    A reported count mismatch suppresses the per-column checks. A column that builds JSON
    of a known shape is also checked against the field's annotation, nested types looked up
    in ``type_defs`` (``json-shape-mismatch``). A column of a composite type whose field is
    annotated with a row type in ``type_defs`` is compared with the fields ``composite``
    gives for its SQL type, the same way.
    """
    from sqlty._type_extractor import check_type_compatibility, field_type

    fields = info.fields
    declared = dict(fields)
    item = "column" if source == "query" else "field"
    counted = (
        f"{source} returns {len(columns)} columns"
        if source == "query"
        else f"{source} has {len(columns)} fields"
    )
    by_name = info.kind != "NamedTuple"
    by_alias = info.kind == "BaseModel"
    unit = {"TypedDict": "keys", "Protocol": "attributes"}.get(info.kind, "fields")
    open_ended = bool(info.extra_items) or by_alias or info.allows_extra
    lo = len(info.required - info.opaque) if by_name else len(fields)
    hi = len(columns) if open_ended else len(fields)
    if not lo <= len(columns) <= hi and rules.is_enabled("column-count-mismatch"):
        expected = f"{len(fields)} fields"
        if open_ended:
            expected = f"{lo} required {unit}"
        elif lo < hi:
            expected = f"{lo} required and {hi} total {unit}"
        return [
            (
                "column-count-mismatch",
                f"{type_name} has {expected} but {counted}",
            )
        ]

    def type_mismatch(field: str, col: _RowColumn) -> list[tuple[str, str]]:
        declared_type = declared.get(field) or f"extra_items {info.extra_items}"
        if col.python_type and not check_type_compatibility(info, field, col.python_type):
            return [
                (
                    "column-type-mismatch",
                    f"{type_name}.{field} is {declared_type} but {source} {item} "
                    f"'{col.name}' infers {col.python_type}",
                )
            ]
        found = _field_diagnostics(
            f"{type_name}.{field}", field_type(info, field), col, rules, f"{source} {item}"
        ) + _json_diagnostics(
            f"{type_name}.{field}", info.annotations.get(field), col, type_defs, info.aliases, rules
        )
        nested = _row_type_of(info.annotations.get(field), type_defs)
        if col.python_type is None and col.sql_type and composite and nested:
            if (fields := composite(col.sql_type)) is not None:
                found += _structural_diagnostics(
                    f"{type_name}.{field}",
                    nested,
                    fields,
                    rules,
                    type_defs,
                    composite,
                    f"composite {col.sql_type.lower()}",
                )
        return found

    if not by_name:
        pairs = list(zip(fields, columns, strict=False))
        return [d for (field, _), col in pairs for d in type_mismatch(field, col)] + [
            (
                "column-name-mismatch",
                f"{type_name}.{field} doesn't match {source} {item} '{col.name}'",
            )
            for (field, _), col in pairs
            if field != col.name
        ]

    admits_extra = info.extra_items or info.allows_extra or info.opaque
    owners = [(col, info.field_for(col.name)) for col in columns]
    known = [(col, field or col.name) for col, field in owners if field or info.extra_items]
    unknown = [col for col, field in owners if field is None and not admits_extra]
    names = {col.name for col in columns}
    missing = [
        info.column_names(key)[0]
        for key, _ in fields
        if key in info.required
        and key not in info.opaque
        and not names.intersection(info.column_names(key))
    ]
    noun = {"TypedDict": "key", "Protocol": "attribute"}.get(info.kind, "field")
    return (
        [d for col, field in known for d in type_mismatch(field, col)]
        + [
            (
                "typeddict-key-mismatch",
                f"{type_name} has no {noun} for {source} {item} '{col.name}'",
            )
            for col in unknown
        ]
        + [
            ("typeddict-key-mismatch", f"{type_name}.{key} is required but missing from {source}")
            for key in missing
        ]
    )
