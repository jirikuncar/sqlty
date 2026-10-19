"""The columns of PostgreSQL's built-in functions that return `record` through OUT parameters.

Reads `pg_proc` on a live server and writes
`rust/crates/sqlty-sql/src/analyzer/pg_record_functions.rs`, which types
`SELECT * FROM pg_stat_file('x')` and the like. A function whose overloads return
different columns, or a column sqlty has no SQL type for, is left out, and so are the
ones `table_functions.rs` reads itself.

    SQLTY_PG_DSN=postgresql://postgres:postgres@localhost:5432/postgres \\
        uv run python scripts/compat/pg_record_functions.py
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT))

import psycopg  # noqa: E402

from sqlty import core  # noqa: E402

TARGET = ROOT / "rust/crates/sqlty-sql/src/analyzer/pg_record_functions.rs"
READ_ELSEWHERE = {
    "json_each",
    "json_each_text",
    "jsonb_each",
    "jsonb_each_text",
    "unnest",
    "pg_input_error_info",
}
QUERY = """
SELECT p.proname,
       array_agg(format_type(t.type, NULL) ORDER BY t.n),
       array_agg(p.proargnames[t.n] ORDER BY t.n)
FROM pg_proc p,
     unnest(p.proallargtypes, p.proargmodes) WITH ORDINALITY AS t(type, mode, n)
WHERE p.pronamespace = 'pg_catalog'::regnamespace
  AND p.prorettype = 'record'::regtype
  AND p.proname NOT LIKE '\\_%'
  AND t.mode IN ('o', 'b', 't')
GROUP BY p.oid, p.proname
ORDER BY p.proname
"""


def sql_type(pg_type: str) -> str | None:
    columns = core.analyze_select(f"SELECT NULL::{pg_type} AS x", dialect="postgres")
    return columns[0].sql_type if columns else None


def main() -> None:
    with psycopg.connect(os.environ["SQLTY_PG_DSN"]) as conn:
        version = conn.execute("SHOW server_version").fetchone()[0]  # type: ignore[index]
        rows = conn.execute(QUERY).fetchall()
    overloads: dict[str, set[tuple[tuple[str, str], ...]]] = {}
    for name, types, names in rows:
        if name not in READ_ELSEWHERE:
            overloads.setdefault(name, set()).add(tuple(zip(names, types, strict=True)))
    arms = []
    for name, shapes in sorted(overloads.items()):
        if len(shapes) != 1:
            continue
        typed = [(column, sql_type(pg_type)) for column, pg_type in shapes.pop()]
        if any(sql is None for _, sql in typed):
            continue
        columns = "".join(f'("{column}", {json.dumps(sql)}), ' for column, sql in typed)
        arms.append(f'        "{name}" => &[{columns}],\n')
    TARGET.write_text(
        f"//! Columns of PostgreSQL {version.split('.')[0]}'s built-in functions that return `record`\n"
        "//! through OUT parameters. Generated from `pg_proc` by\n"
        "//! `scripts/compat/pg_record_functions.py`; do not edit.\n\n"
        "/// The `(name, SQL type)` columns of the built-in function `name` (lower case).\n"
        "pub(super) fn record_columns(name: &str) -> Option<&'static [(&'static str, &'static str)]> {\n"
        "    Some(match name {\n"
        f"{''.join(arms)}"
        "        _ => return None,\n"
        "    })\n"
        "}\n"
    )
    subprocess.run(["rustfmt", "--edition", "2021", str(TARGET)], check=True)


if __name__ == "__main__":
    main()
