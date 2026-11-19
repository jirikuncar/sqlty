"""The Python types sqlty gives SQL types: well-formed however deeply nested, and keeping
the values of numbers, dates and durations."""

from __future__ import annotations

import os
from functools import cache

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty import core

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))

STDLIB = {
    "int",
    "float",
    "str",
    "bytes",
    "bool",
    "decimal.Decimal",
    "datetime.date",
    "datetime.time",
    "datetime.datetime",
    "datetime.timedelta",
    "uuid.UUID",
    "ipaddress.IPv4Address",
    "ipaddress.IPv6Address",
    "ipaddress.IPv4Interface",
    "ipaddress.IPv6Interface",
    "ipaddress.IPv4Network",
    "ipaddress.IPv6Network",
    "typing.Any",
}

SCALARS = (
    "SMALLINT",
    "INTEGER",
    "BIGINT",
    "REAL",
    "DOUBLE",
    "DOUBLE PRECISION",
    "DECIMAL(10, 2)",
    "TEXT",
    "VARCHAR",
    "BOOLEAN",
    "BYTEA",
    "BLOB",
    "DATE",
    "TIME",
    "TIMESTAMP",
    "TIMESTAMPTZ",
    "INTERVAL",
    "UUID",
    "JSON",
    "JSONB",
    "INET",
    "CIDR",
)

# fmt: off
KEPT = [
    ("postgres", "BIGINT", "int"), ("mysql", "SIGNED", "int"),
    ("sqlite", "INTEGER", "int"), ("duckdb", "HUGEINT", "int"),
    ("postgres", "DECIMAL(10, 2)", "decimal.Decimal"),
    ("mysql", "DECIMAL(10, 2)", "decimal.Decimal"),
    ("duckdb", "DECIMAL(10, 2)", "decimal.Decimal"),
    ("sqlite", "DECIMAL(10, 2)", "float"),
    ("postgres", "DATE", "datetime.date"), ("mysql", "DATE", "datetime.date"),
    ("duckdb", "DATE", "datetime.date"), ("sqlite", "DATE", "str"),
    ("postgres", "TIMESTAMPTZ", "datetime.datetime"),
    ("duckdb", "TIMESTAMP", "datetime.datetime"), ("sqlite", "TIMESTAMP", "str"),
    ("postgres", "INTERVAL", "datetime.timedelta"),
    ("duckdb", "INTERVAL", "datetime.timedelta"),
    ("mysql", "TIME", "datetime.timedelta"),
]
# fmt: on


@pytest.mark.parametrize(("dialect", "spelling", "python_type"), KEPT)
def test_types_keep_their_values(dialect: str, spelling: str, python_type: str) -> None:
    assert _python(spelling, dialect) == python_type


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=[HealthCheck.too_slow])
@given(st.data())
def test_nested_types_are_well_formed(data: st.DataObject) -> None:
    dialect = data.draw(st.sampled_from(("postgres", "duckdb")))
    spelling = data.draw(_types(dialect))
    if (python_type := _python(spelling, dialect)) is not None:
        assert _well_formed(python_type), f"{dialect}: {spelling} is {python_type}"


def _types(dialect: str) -> st.SearchStrategy[str]:
    scalar = st.sampled_from(_scalars(dialect))
    if dialect == "postgres":
        arrays = st.sampled_from(("[]", "[3]"))
        return st.recursive(
            scalar, lambda inner: st.tuples(inner, arrays).map("".join), max_leaves=3
        )

    def containers(inner: st.SearchStrategy[str]) -> st.SearchStrategy[str]:
        fields = st.lists(inner, min_size=1, max_size=3).map(
            lambda types: ", ".join(f"f{i} {t}" for i, t in enumerate(types))
        )
        return st.one_of(
            inner.map(lambda t: f"{t}[]"),
            inner.map(lambda t: f"{t}[3]"),
            fields.map(lambda f: f"STRUCT({f})"),
            fields.map(lambda f: f"UNION({f})"),
            st.tuples(scalar, inner).map(lambda kv: f"MAP({kv[0]}, {kv[1]})"),
        )

    return st.recursive(scalar, containers, max_leaves=6)


@cache
def _scalars(dialect: str) -> list[str]:
    return [name for name in SCALARS if _python(name, dialect) is not None]


@cache
def _schema(dialect: str) -> core.Schema:
    return core.load_schema("CREATE TABLE t (c TEXT NOT NULL)", dialect=dialect)


def _python(spelling: str, dialect: str) -> str | None:
    query = f"SELECT CAST(c AS {spelling}) AS c FROM t"
    columns = core.analyze_select(query, dialect=dialect, schema_sql=_schema(dialect))
    python_type = columns[0].python_type if columns else None
    # A MySQL cast of text to a date can be NULL.
    return python_type and python_type.removesuffix(" | None")


def _well_formed(python_type: str) -> bool:
    """Standard library classes, no empty ``tuple[]``, and unions of two or more
    distinct members, none a union itself."""
    members = _split(python_type, "|")
    if len(members) > 1:
        return len(set(members)) == len(members) and all(map(_well_formed, members))
    if python_type == "list" or python_type in STDLIB:
        return True
    head, _, rest = python_type.partition("[")
    if not rest.endswith("]"):
        return False
    args = _split(rest[:-1], ",")
    match head, args:
        case "list", [item]:
            return _well_formed(item)
        case "tuple", [item, "..."]:
            return _well_formed(item)
        case "tuple", [_, *_]:
            return all(map(_well_formed, args))
        case "dict", [key, value]:
            return _well_formed(key) and _well_formed(value)
    return False


def _split(text: str, separator: str) -> list[str]:
    parts, depth, start = [], 0, 0
    for i, char in enumerate(text):
        if char == "[":
            depth += 1
        elif char == "]":
            depth -= 1
        elif char == separator and depth == 0:
            parts.append(text[start:i])
            start = i + 1
    parts.append(text[start:])
    return [part.strip() for part in parts]
