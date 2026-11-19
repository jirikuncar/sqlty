"""Property-based tests of ``foreign-key-mismatch``: key comparisons in random query shapes.

An independent oracle judges each comparison from the schema; the rule must report exactly
what it does, wherever the comparison sits: ``ON``, ``WHERE``, ``HAVING``, joins of every
kind, ``LATERAL``, subqueries, CTEs, derived tables, ``UNION`` branches.
``SQLTY_FUZZ_EXAMPLES`` raises the number of examples (default 100)::

    SQLTY_FUZZ_EXAMPLES=5000 uv run pytest tests/test_foreign_keys_fuzz.py -n 0 -p no:randomly
"""

from __future__ import annotations

import os
from collections.abc import Callable

from hypothesis import assume, given, settings
from hypothesis import strategies as st

from sqlty import core

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "100"))
fuzz = settings(max_examples=EXAMPLES, deadline=None)

SCHEMA = """
CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER REFERENCES customers (id),
    product_id INTEGER REFERENCES products (id),
    total INTEGER
);
CREATE TABLE vip (customer_id INTEGER PRIMARY KEY REFERENCES customers (id));
CREATE TABLE vip_notes (id INTEGER PRIMARY KEY, vip_id INTEGER REFERENCES vip (customer_id));
CREATE TABLE employees (id INTEGER PRIMARY KEY, manager_id INTEGER REFERENCES employees (id));
CREATE TABLE events (id BIGINT, customer_id INTEGER, product_id INTEGER);
CREATE TABLE warehouses (region TEXT, code TEXT, PRIMARY KEY (region, code));
CREATE TABLE stock (
    id INTEGER PRIMARY KEY,
    region TEXT,
    code TEXT,
    FOREIGN KEY (region, code) REFERENCES warehouses (region, code)
);
"""
COLUMNS = {
    "customers": ["id", "name"],
    "products": ["id", "name"],
    "orders": ["id", "customer_id", "product_id", "total"],
    "vip": ["customer_id"],
    "vip_notes": ["id", "vip_id"],
    "employees": ["id", "manager_id"],
    "events": ["id", "customer_id", "product_id"],
}
KEYS = {
    ("orders", "customer_id"): ("customers", "id"),
    ("orders", "product_id"): ("products", "id"),
    ("vip", "customer_id"): ("customers", "id"),
    ("vip_notes", "vip_id"): ("vip", "customer_id"),
    ("employees", "manager_id"): ("employees", "id"),
}
PRIMARY = {(table, "id") for table in COLUMNS if table not in ("vip", "events")} | {
    ("vip", "customer_id")
}

Column = tuple[str, str]


def entity(column: Column) -> Column | None:
    """The key column a column's values are keys of, following foreign keys to the end."""
    while column in KEYS:
        column = KEYS[column]
    return column if column in PRIMARY else None


def expected(left: Column, right: Column) -> str | None:
    """What the rule reports for ``left`` compared with ``right``, by the rule page."""
    if left not in KEYS and right not in KEYS:
        return None
    same_keys = KEYS.get(left) is not None and KEYS.get(left) == KEYS.get(right)
    namesake = left[1] == right[1] and any(
        side not in KEYS and entity(side) is None for side in (left, right)
    )
    if (
        left == right
        or KEYS.get(left) == right
        or KEYS.get(right) == left
        or same_keys
        or entity(left) is not None
        and entity(left) == entity(right)
        or namesake
    ):
        return None
    key, other = (left, right) if left in KEYS else (right, left)
    target = KEYS[key]
    return f"{key[0]}.{key[1]} references {target[0]}.{target[1]}, not {other[0]}.{other[1]}"


NOISE = ["{x}.id > 0", "{y}.id IS NOT NULL", "1 = 1", "{x}.id IN (1, 2)"]
# Each form, and whether its left operand is ``b``.
FORMS = [
    ("{a} = {b}", False),
    ("{b} = {a}", True),
    ("{a} <> {b}", False),
    ("({a} = {b})", False),
    ("NOT ({a} = {b})", False),
    ("({a}, 1) = ({b}, 1)", False),
    ("(1, {b}) = (1, {a})", True),
    ("{a} IN ({b}, 3)", False),
    ("{a} IN ({b})", False),
    ("{a} NOT IN ({b})", False),
]


def joined(draw: st.DrawFn, terms: list[str], ops: list[str]) -> str:
    terms = draw(st.permutations(terms))
    cond = terms[0]
    for term in terms[1:]:
        cond = f"{cond} {draw(st.sampled_from(ops))} {term}"
    return cond


def shapes(x: str, y: str, tx: str, ty: str, cond: str) -> list[str]:
    """A query ``SELECT 1 ...`` comparing source ``x`` of ``tx`` with ``y`` of ``ty``."""
    return [
        *(
            f"SELECT 1 FROM {tx} {x} {kind} {ty} {y} ON {cond}"
            for kind in ("JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN")
        ),
        f"SELECT 1 FROM {tx} {x}, {ty} {y} WHERE {cond}",
        f"SELECT 1 FROM {tx} {x} CROSS JOIN {ty} {y} WHERE {cond}",
        f"SELECT 1 FROM {tx} {x} JOIN {ty} {y} ON true WHERE {cond}",
        f"SELECT 1 FROM {tx} {x}, {ty} {y} GROUP BY {x}.id, {y}.id HAVING {cond}",
        f"SELECT 1 FROM {tx} {x} WHERE EXISTS (SELECT 1 FROM {ty} {y} WHERE {cond})",
        f"SELECT 1 FROM {tx} {x} WHERE NOT EXISTS (SELECT 1 FROM {ty} {y} WHERE {cond})",
        f"SELECT 1 FROM {tx} {x} WHERE 1 IN (SELECT 1 FROM {ty} {y} WHERE {cond})",
        f"SELECT 1 FROM {tx} {x} JOIN LATERAL (SELECT 1 AS one FROM {ty} {y} WHERE {cond}) l{y} ON true",
        f"SELECT 1 FROM {tx} {x} CROSS JOIN LATERAL (SELECT 1 AS one FROM {ty} {y} WHERE {cond}) l{y}",
        f"SELECT (SELECT 1 FROM {ty} {y} WHERE {cond} LIMIT 1) FROM {tx} {x}",
        f"SELECT 1 FROM {tx} {x} JOIN {ty} {y} ON EXISTS (SELECT 1 WHERE {cond})",
        f"SELECT 1 FROM {tx} {x} JOIN {ty} {y} ON true WHERE {x}.id > 0 AND ({cond})",
    ]


WRAPPERS: list[Callable[[str, int], str]] = [
    lambda q, n: q,
    lambda q, n: f"WITH c{n} AS ({q}) SELECT 1 FROM c{n}",
    lambda q, n: (
        f"WITH RECURSIVE r{n} AS ({q} UNION ALL SELECT 1 FROM r{n} WHERE false) SELECT 1 FROM r{n}"
    ),
    lambda q, n: f"SELECT 1 FROM ({q}) d{n}",
    lambda q, n: f"SELECT 1 FROM customers z{n} JOIN LATERAL ({q}) d{n} ON true",
    lambda q, n: f"SELECT 1 WHERE EXISTS ({q})",
    lambda q, n: f"SELECT 1 FROM customers z{n} WHERE 1 IN ({q})",
    lambda q, n: f"SELECT ({q} LIMIT 1)",
    lambda q, n: f"{q} UNION ALL SELECT 1",
    lambda q, n: f"SELECT 1 UNION SELECT * FROM ({q}) u{n}",
]


def nest(draw: st.DrawFn, sql: str) -> str:
    for n in range(draw(st.integers(0, 2))):
        sql = draw(st.sampled_from(WRAPPERS))(sql, n)
    return sql


# Names are judged where the dialect's name rules are modelled: not in DuckDB.
JUDGED = {"postgres", "sqlite", "mysql"}
UNSUPPORTED = {"sqlite": ("LATERAL", "RIGHT JOIN", "FULL JOIN"), "mysql": ("FULL JOIN",)}
DIALECTS = st.sampled_from(["postgres", "duckdb", "sqlite", "mysql"])


def supported(sql: str, dialect: str) -> bool:
    return not any(word in sql for word in UNSUPPORTED.get(dialect, ()))


def found(sql: str, dialect: str = "postgres") -> set[str]:
    marker = "-- sql" if dialect == "mysql" else "--sql"
    source = f"q = {(marker + chr(10) + sql)!r}\n"
    diagnostics = core.check_source(source, dialect=dialect, schema_sql=SCHEMA)
    assert not [d for d in diagnostics if d["rule"] == "internal-error"], sql
    return {d["message"] for d in diagnostics if d["rule"] == "foreign-key-mismatch"}


@st.composite
def comparison_of_tables(draw: st.DrawFn) -> tuple[str, set[str]]:
    """The tables are drawn first, so the columns compared belong to them."""
    tx = draw(st.sampled_from(sorted(COLUMNS)))
    ty = draw(st.sampled_from(sorted(COLUMNS)))
    pairs = draw(
        st.lists(
            st.tuples(st.sampled_from(COLUMNS[tx]), st.sampled_from(COLUMNS[ty])),
            min_size=1,
            max_size=3,
        )
    )
    terms, reports = [], set()
    for a, b in pairs:
        form, swapped = draw(st.sampled_from(FORMS))
        terms.append(form.format(a=f"x.{a}", b=f"y.{b}"))
        left, right = (ty, b), (tx, a)
        if not swapped:
            left, right = right, left
        if message := expected(left, right):
            reports.add(message)
    terms += [n.format(x="x", y="y") for n in draw(st.lists(st.sampled_from(NOISE), max_size=2))]
    cond = joined(draw, terms, ["AND", "OR"])
    # A derived table hides the keys of its table: nothing is judged.
    source = draw(
        st.sampled_from(["{t}", "{t}", "{t}", "(SELECT * FROM {t})", "(SELECT {t}.* FROM {t})"])
    )
    if source != "{t}":
        reports = set()
    sql = draw(st.sampled_from(shapes("x", "y", tx, source.format(t=ty), cond)))
    return nest(draw, sql), reports


@fuzz
@given(comparison_of_tables(), DIALECTS)
def test_single_column_keys_in_any_shape(case: tuple[str, set[str]], dialect: str) -> None:
    sql, reports = case
    assume(supported(sql, dialect))
    assert found(sql, dialect) == (reports if dialect in JUDGED else set()), sql


@st.composite
def composite(draw: st.DrawFn) -> tuple[str, set[str]]:
    """``stock`` compared with one ``warehouses`` row on some of its key, in any shape."""
    columns = draw(st.sets(st.sampled_from(["region", "code"]), min_size=1))
    pinned = {"region", "code"} - columns if draw(st.booleans()) else set()
    terms = [f"s.{c} = w.{c}" for c in sorted(columns)]
    terms += [f"s.{c} = 'k' AND w.{c} = 'k'" for c in sorted(pinned)]
    terms += [n.format(x="s", y="w") for n in draw(st.lists(st.sampled_from(NOISE), max_size=2))]
    cond = joined(draw, terms, ["AND"])
    sql = nest(draw, draw(st.sampled_from(shapes("s", "w", "stock", "warehouses", cond))))
    whole = columns | pinned == {"region", "code"}
    reports = (
        set()
        if whole
        else {
            f"stock (region, code) references warehouses (region, code), compared with w on {', '.join(sorted(columns))} only"
        }
    )
    return sql, reports


@fuzz
@given(composite())
def test_composite_keys_in_any_shape(case: tuple[str, set[str]]) -> None:
    sql, reports = case
    assert found(sql) == reports, sql


JOINS = ["JOIN", "INNER JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN", "CROSS JOIN", ","]


@st.composite
def join_chain(draw: st.DrawFn) -> tuple[str, set[str]]:
    """Three to five sources joined in a chain, each condition comparing sources before it,
    with ``USING`` where one source before has the column."""
    sources = [
        (f"s{i}", draw(st.sampled_from(sorted(COLUMNS)))) for i in range(draw(st.integers(3, 5)))
    ]
    reports: set[str] = set()

    def compare(visible: list[tuple[str, str]]) -> str:
        (x, tx), (y, ty) = draw(st.lists(st.sampled_from(visible), min_size=2, max_size=2))
        a, b = draw(st.sampled_from(COLUMNS[tx])), draw(st.sampled_from(COLUMNS[ty]))
        form, swapped = draw(st.sampled_from(FORMS))
        left, right = ((ty, b), (tx, a)) if swapped else ((tx, a), (ty, b))
        if message := expected(left, right):
            reports.add(message)
        return form.format(a=f"{x}.{a}", b=f"{y}.{b}")

    sql = f"SELECT 1 FROM {sources[0][1]} {sources[0][0]}"
    where = []
    for i, (alias, table) in enumerate(sources[1:], 1):
        visible = sources[: i + 1]
        kind = draw(st.sampled_from(JOINS))
        shared = [
            column
            for column in COLUMNS[table]
            if sum(column in COLUMNS[t] for _, t in sources[:i]) == 1
        ]
        if kind in (",", "CROSS JOIN"):
            sql += f"{' ' if kind == 'CROSS JOIN' else ''}{kind} {table} {alias}"
        elif shared and draw(st.booleans()):
            column = draw(st.sampled_from(shared))
            other = next(t for _, t in sources[:i] if column in COLUMNS[t])
            if message := expected((other, column), (table, column)):
                reports.add(message)
            sql += f" {kind} {table} {alias} USING ({column})"
        else:
            terms = [compare(visible) for _ in range(draw(st.integers(1, 2)))]
            sql += f" {kind} {table} {alias} ON {joined(draw, terms, ['AND', 'OR'])}"
        if draw(st.booleans()):
            where.append(compare(visible))
    if where:
        sql += f" WHERE {joined(draw, where, ['AND', 'OR'])}"
    return nest(draw, sql), reports


@fuzz
@given(join_chain(), DIALECTS)
def test_join_chains(case: tuple[str, set[str]], dialect: str) -> None:
    sql, reports = case
    assume(supported(sql, dialect))
    assert found(sql, dialect) == (reports if dialect in JUDGED else set()), sql


KEY = ("region", "code")


@st.composite
def composite_rows(draw: st.DrawFn) -> tuple[str, set[str]]:
    """Each ``stock`` key column compared with a column of ``w`` or ``v``, pinned, NULL or
    left out, and the reports the rule page gives for that."""
    pairs, pins, nulls = [], set(), False
    terms = []
    for column in KEY:
        way = draw(st.sampled_from(["compare", "pin", "pin-other", "null", "none"]))
        if way == "compare":
            alias, other = draw(st.sampled_from(["w", "v"])), draw(st.sampled_from(KEY))
            pairs.append((column, alias, other))
            terms.append(
                draw(
                    st.sampled_from(["s.{c} = {a}.{o}", "{a}.{o} = s.{c}", "(s.{c}) IN ({a}.{o})"])
                ).format(c=column, a=alias, o=other)
            )
        elif way.startswith("pin"):
            value = draw(st.sampled_from(["'k'", "$1", "2"]))
            other = value if way == "pin" else draw(st.sampled_from(["'j'", "$2", "NULL"]))
            terms.append(f"s.{column} = {value} AND w.{column} = {other}")
            if way == "pin" or other == value:
                pins.add(column)
        elif way == "null":
            terms.append(f"s.{column} IS NULL")
            nulls = True
    terms += [
        n.format(x="s", y="w") for n in draw(st.lists(st.sampled_from(NOISE[2:]), max_size=1))
    ]
    reports = set()
    straight = [(c, a) for c, a, o in pairs if c == o]
    for column, _, other in pairs:
        if column != other:
            reports.add(f"stock.{column} references warehouses.{column}, not warehouses.{other}")
    rows = {a for _, a in straight}
    if len(rows) > 1:
        compared = " and ".join(f"{a}.{c}" for c, a in straight)
        reports.add(f"stock (region, code) references one warehouses row, compared with {compared}")
    elif straight and not nulls:
        (alias,) = rows
        columns = {c for c, _ in straight}
        pinned = pins if alias == "w" else set()
        if columns | pinned != set(KEY):
            only = ", ".join(sorted(columns))
            reports.add(
                f"stock (region, code) references warehouses (region, code), compared with {alias} on {only} only"
            )
    cond = " AND ".join(terms) or "true"
    if draw(st.booleans()):
        cond = f"({cond}) OR {draw(st.sampled_from(['1 = 2', 's.id IS NULL']))}"
    sql = f"SELECT 1 FROM stock s, warehouses w, warehouses v WHERE {cond}"
    return nest(draw, sql), reports


@fuzz
@given(composite_rows())
def test_composite_keys_across_rows(case: tuple[str, set[str]]) -> None:
    sql, reports = case
    assert found(sql) == reports, sql
