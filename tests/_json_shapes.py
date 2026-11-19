"""The JSON shapes of query columns as readable text, for ``-- json:`` assertions."""

from __future__ import annotations

import json
from typing import Any

from sqlty import _core
from sqlty._core import Schema


def json_shapes(
    sql: str, *, dialect: str = "postgres", schema_sql: str | Schema | None = None
) -> list[tuple[str, str]]:
    """``(column, shape)`` of each column that builds JSON of a known shape.

    Objects are ``{key: shape}`` (``key?`` when it may be missing), arrays ``[shape]``,
    maps ``{[key]: shape}``, and a value that can be ``null`` ends in ``| null``.
    """
    columns = _core.analyze_columns(sql, dialect=dialect, schema_sql=schema_sql)
    return [(c[0], _render(json.loads(c[4]))) for c in columns or [] if c[4] is not None]


def _render(shape: Any) -> str:
    if isinstance(shape, str):
        return shape
    kind, body = next(iter(shape.items()))
    if kind == "array":
        return f"[{_render_value(body)}]"
    if kind == "map":
        return f"{{[key]: {_render_value(body)}}}"
    fields = ", ".join(
        f"{f['name']}{'?' if f['optional'] else ''}: {_render_value(f['value'])}" for f in body
    )
    return f"{{{fields}}}"


def _render_value(value: dict[str, Any]) -> str:
    text = _render(value["shape"])
    return f"{text} | null" if value["nullable"] else text
