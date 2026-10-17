"""Rows from SQLAlchemy 2.0: its column types, functions and operators per dialect.

Column axis: every `TypeEngine` of `sqlalchemy.types` and of the dialect's module that
builds without arguments, compiled to DDL, with its `python_type`. Function axis: the
generic SQL functions over typed columns, with `func.<name>(...).type.python_type`.
Operator axis: binary operators between typed columns, with the result's `python_type`.
"""

from __future__ import annotations

import inspect
from collections.abc import Iterator

from harness import Row

DIALECTS = {"postgres": "postgresql", "mysql": "mysql", "sqlite": "sqlite"}
# The typed table the function and operator axes read.
COLUMNS = {
    "i": "Integer",
    "b": "BigInteger",
    "n": "Numeric",
    "f": "Float",
    "s": "String",
    "d": "Date",
    "ts": "DateTime",
    "iv": "Interval",
    "bo": "Boolean",
}


def rows() -> Iterator[Row]:
    import sqlalchemy as sa
    from sqlalchemy import types
    from sqlalchemy.dialects import mysql, postgresql, sqlite

    version = sa.__version__
    modules = {"postgres": postgresql, "mysql": mysql, "sqlite": sqlite}
    for dialect, module in modules.items():
        engine_dialect = module.dialect()
        yield from _columns(version, dialect, engine_dialect, [types, module])
        table = _table(sa, engine_dialect)
        yield from _functions(sa, version, dialect, engine_dialect, table)
        yield from _operators(sa, version, dialect, engine_dialect, table)


def _python(type_engine: object) -> str | None:
    try:
        python = type_engine.python_type  # type: ignore[attr-defined]
    except NotImplementedError:
        return None
    # SQLAlchemy 2.1 says `object` where 2.0 raised: it has no Python type either way.
    if python is object:
        return None
    module = python.__module__
    return python.__name__ if module == "builtins" else f"{module}.{python.__qualname__}"


def _columns(
    version: str, dialect: str, engine_dialect: object, modules: list[object]
) -> Iterator[Row]:
    from sqlalchemy.types import TypeEngine

    seen: set[str] = set()
    for module in modules:
        for name, cls in sorted(vars(module).items()):
            if not (inspect.isclass(cls) and issubclass(cls, TypeEngine)) or name.startswith("_"):
                continue
            try:
                type_engine = cls()
                ddl = type_engine.compile(dialect=engine_dialect)
            except Exception:  # a type that needs arguments or this dialect lacks
                continue
            if ddl in seen:
                continue
            seen.add(ddl)
            yield Row(
                "sqlalchemy",
                version,
                dialect,
                "column",
                ddl,
                _python(type_engine),
                f"{module.__name__}.{name}",
            )


def _table(sa: object, engine_dialect: object) -> object:
    metadata = sa.MetaData()

    def build(kind: str) -> object:
        # MySQL's VARCHAR needs a length.
        return sa.String(100) if kind == "String" else getattr(sa, kind)()

    columns = [sa.Column(name, build(kind), nullable=False) for name, kind in COLUMNS.items()]
    return sa.Table("t", metadata, *columns)


def _schema(sa: object, table: object, engine_dialect: object) -> str:
    from sqlalchemy.schema import CreateTable

    return str(CreateTable(table).compile(dialect=engine_dialect)).strip() + ";"


def _sql(expr: object, engine_dialect: object) -> str:
    compiled = expr.compile(dialect=engine_dialect, compile_kwargs={"literal_binds": True})
    return str(compiled).replace("t.", "")


def _functions(
    sa: object, version: str, dialect: str, engine_dialect: object, table: object
) -> Iterator[Row]:
    func, c = sa.func, table.c
    schema = _schema(sa, table, engine_dialect)
    calls = {
        "count": func.count(c.i),
        "sum(int)": func.sum(c.i),
        "sum(numeric)": func.sum(c.n),
        "avg(int)": func.avg(c.i),
        "min(text)": func.min(c.s),
        "max(date)": func.max(c.d),
        "now": func.now(),
        "current_date": func.current_date(),
        "current_timestamp": func.current_timestamp(),
        "localtime": func.localtime(),
        "current_user": func.current_user(),
        "coalesce": func.coalesce(c.i, c.b),
        "char_length": func.char_length(c.s),
        "lower": func.lower(c.s),
        "upper": func.upper(c.s),
        "concat": func.concat(c.s, c.s),
        "random": func.random(),
        "abs": func.abs(c.n),
        "round": func.round(c.n),
        "percent_rank": func.percent_rank(),
        "cume_dist": func.cume_dist(),
        "rank": func.rank(),
        "row_number": func.row_number(),
        "array_agg": func.array_agg(c.i),
    }
    for name, call in calls.items():
        try:
            sql = _sql(call, engine_dialect)
        except Exception:  # a function this dialect can't compile
            continue
        yield Row(
            "sqlalchemy",
            version,
            dialect,
            "expression",
            sql,
            _python(call.type),
            f"func.{name}",
            schema,
        )


def _operators(
    sa: object, version: str, dialect: str, engine_dialect: object, table: object
) -> Iterator[Row]:
    c = table.c
    schema = _schema(sa, table, engine_dialect)
    pairs = {
        "int + int": c.i + c.i,
        "int + bigint": c.i + c.b,
        "int + numeric": c.i + c.n,
        "int + float": c.i + c.f,
        "int / int": c.i / c.i,
        "int % numeric": c.i % c.n,
        "numeric * numeric": c.n * c.n,
        "text || text": c.s + c.s,
        "int || text": c.i.concat(c.s),
        "date + interval": c.d + c.iv,
        "datetime - datetime": c.ts - c.ts,
        "datetime - interval": c.ts - c.iv,
        "int = int": c.i == c.i,
        "text like": c.s.like("a%"),
        "bool and": sa.and_(c.bo, c.bo),
    }
    for name, expr in pairs.items():
        try:
            sql = _sql(expr, engine_dialect)
        except Exception:  # an operator this dialect can't compile
            continue
        yield Row(
            "sqlalchemy", version, dialect, "expression", sql, _python(expr.type), name, schema
        )
