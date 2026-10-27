"""Pydantic agrees with `check` on which columns its types accept.

For each type of the static table (``pydantic_types``), a model with one field per column
type is checked against a query of each column, and ``TypeAdapter(T).validate_python``
is run on values a driver returns for that column. Pydantic is imported here, never by
sqlty.
"""

from __future__ import annotations

import datetime
import decimal
import ipaddress
import re
import typing
import uuid

import pydantic
import pytest

from sqlty._type_extractor import pydantic_types
from sqlty.core import check_source

UTC = datetime.UTC
TEXT = [
    "a@b.co",
    "A <a@b.co>",
    "https://a.co",
    "postgres://u:p@h:5432/db",
    "amqp://h",
    "clickhouse+native://u@h/db",
    "cockroachdb://u@h/db",
    "kafka://h:9092",
    "mariadb://u@h/db",
    "mongodb://h/db",
    "mysql://u@h/db",
    "nats://h:4222",
    "redis://h/0",
    "snowflake://u:p@account/db/schema?warehouse=w&role=r",
    "ws://a.co",
    "file:///x",
    "ftp://a.co",
    "eA==",
    "4242424242424242",
    "127.0.0.1",
    "127.0.0.0/8",
    "1kb",
    "{}",
    "x",
]
# Column -> (SQL, the values a driver returns, by Python type). Several values of one
# type stand for any value of it: a constrained type accepts one of them.
COLUMNS: dict[str, tuple[str, list[list[object]]]] = {
    "integer": ("1::INTEGER", [[1, 0, -1]]),
    "float8": ("1.5::FLOAT8", [[1.5, -1.5, 1.0, -1.0]]),
    "numeric": ("1::NUMERIC", [[decimal.Decimal(1), decimal.Decimal(-1), decimal.Decimal("1.5")]]),
    "boolean": ("TRUE", [[True, False]]),
    "text": ("'x'::TEXT", [TEXT]),
    "bytea": ("'\\x00'::BYTEA", [[b"eA==", b"1", b"x"]]),
    "array": ("ARRAY[1]", [[[1]]]),
    "date": ("'2000-01-01'::DATE", [[datetime.date(2000, 1, 1), datetime.date(2100, 1, 1)]]),
    "timestamp": (
        "now()",
        [
            [
                datetime.datetime(2000, 1, 1),
                datetime.datetime(2100, 1, 1),
                datetime.datetime(2000, 1, 1, tzinfo=UTC),
                datetime.datetime(2100, 1, 1, tzinfo=UTC),
            ]
        ],
    ),
    "time": ("'01:00'::TIME", [[datetime.time(1)]]),
    "interval": ("'1 day'::INTERVAL", [[datetime.timedelta(1)]]),
    "uuid": ("gen_random_uuid()", [[uuid.UUID(int=1, version=v) for v in range(1, 9)]]),
    "inet": (
        "'1.1.1.1'::INET",
        [[ipaddress.IPv4Address("1.1.1.1")], [ipaddress.IPv4Interface("1.1.1.0/24")]],
    ),
    "cidr": ("'1.1.1.0/24'::CIDR", [[ipaddress.IPv4Network("1.1.1.0/24")]]),
}


_IP = {"IPvAnyAddress", "IPvAnyInterface", "IPvAnyNetwork"}
_FLOATS = {name for name, (tp, _) in pydantic_types.TYPES.items() if tp is float} | {"confloat"}
_NUMBERS = {name for name, (tp, _) in pydantic_types.TYPES.items() if tp is int} | _FLOATS

# Where pydantic and `check` disagree, and why: (reason, types, columns).
EXCEPTIONS = [
    (
        "pydantic parses text and bytes into numbers, sizes and times, and converts "
        "between them; a text column may hold any",
        _NUMBERS
        | {
            "AwareDatetime",
            "PastDatetime",
            "ByteSize",
            "conint",
            "condecimal",
            "conbytes",
            "constr",
        },
        {"text", "bytea"},
    ),
    (
        "pydantic reads a number as a Unix time",
        {"AwareDatetime", "PastDate", "PastDatetime", "condate"},
        {"integer", "float8", "numeric"},
    ),
    (
        "pydantic takes a Decimal for a float; `check` does not, as for `float`",
        _FLOATS,
        {"numeric"},
    ),
    (
        "pydantic reads an integer or bytes as a packed address",
        _IP,
        {"integer", "boolean", "bytea"},
    ),
    (
        "an INET value with a prefix is an interface, which derives from an address",
        {"IPvAnyAddress"},
        {"inet"},
    ),
    ("a date has no timezone", {"AwareDatetime"}, {"date"}),
    (
        "a JsonValue takes no Decimal, which lax `check` takes for an int",
        {"JsonValue"},
        {"numeric"},
    ),
    (
        "`check` takes a bool for a number: none is negative, and pydantic takes none for a Decimal",
        {"NegativeInt", "NegativeFloat", "condecimal"},
        {"boolean"},
    ),
]


def _types() -> dict[str, tuple[str, typing.Any]]:
    """Each type of the table, by name: its annotation, and the type pydantic builds for it."""
    found = {
        name: (f"pydantic.{name}", getattr(pydantic, name))
        for name in [*pydantic_types.TYPES, *pydantic_types.NETWORKS]
    }
    found |= {
        name: (f"pydantic.{name}(int)", getattr(pydantic, name)(int))
        if name == "conlist"
        else (f"pydantic.{name}()", getattr(pydantic, name)())
        for name in pydantic_types.CONSTRAINED
    }
    found |= {
        name: (f"pydantic.{name}[int]", getattr(pydantic, name)[int])
        for name in pydantic_types.GENERIC
    }
    return found


def _rejected(annotation: str) -> set[str]:
    fields = "\n".join(f"    {column}: {annotation}" for column in COLUMNS)
    select = ", ".join(f"{sql} AS {column}" for column, (sql, _) in COLUMNS.items())
    source = f'import pydantic\nfrom sqlty import SQL\nclass R(pydantic.BaseModel):\n{fields}\nq = SQL[R]("--sql\\nSELECT {select}")\n'
    found = check_source(source)
    assert {d["rule"] for d in found} <= {"column-type-mismatch"}, found
    return {m.group(1) for d in found if (m := re.match(r"R\.(\w+) is", str(d["message"])))}


def _accepts(tp: typing.Any, values: list[object]) -> bool:
    adapter = pydantic.TypeAdapter(tp)
    for value in values:
        try:
            adapter.validate_python(value)
        except pydantic.ValidationError:
            continue
        return True
    return False


# Feeding True to an IPv*Network trips a Python 3.15 deprecation inside `ipaddress`.
@pytest.mark.filterwarnings("ignore::DeprecationWarning:ipaddress")
@pytest.mark.filterwarnings("ignore::pydantic.warnings.PydanticDeprecatedSince20")
@pytest.mark.parametrize("name", sorted(_types()))
def test_pydantic_agrees(name: str) -> None:
    annotation, tp = _types()[name]
    rejected = _rejected(annotation)
    disagree = {
        column: "pydantic accepts" if column in rejected else "pydantic rejects"
        for column, (_, groups) in COLUMNS.items()
        if all(_accepts(tp, group) for group in groups) == (column in rejected)
    }
    excused = {column for _, names, columns in EXCEPTIONS if name in names for column in columns}
    assert disagree.keys() <= excused, {c: disagree[c] for c in disagree.keys() - excused}


def test_table_covers_pydantic() -> None:
    """Every public type of pydantic is in the table or left unknown on purpose."""
    from pydantic import networks, types

    from sqlty._type_extractor.ast_fields import _STRICT_TYPES

    table = [*pydantic_types.TYPES, *pydantic_types.CONSTRAINED, *pydantic_types.GENERIC]
    assert sorted(types.__all__) == sorted([*table, *_STRICT_TYPES, *pydantic_types.UNKNOWN_TYPES])
    assert sorted(networks.__all__) == sorted(
        [*pydantic_types.NETWORKS, *pydantic_types.UNKNOWN_NETWORKS]
    )
