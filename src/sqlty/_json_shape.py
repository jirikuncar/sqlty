"""The shape of the JSON a query builds, as readable text.

The engine reports the shape of a ``jsonb_build_object()``, ``jsonb_agg()`` or ``to_jsonb()``
column as JSON text (see ``rust/crates/sqlty-sql/src/json.rs``); ``json-shape-mismatch``
compares it with the row type in Rust (``sqlty_python::rows::json_shape``).
"""

from __future__ import annotations

import json
import typing

if typing.TYPE_CHECKING:
    from collections.abc import Mapping


def render(shape: str) -> str:
    """The JSON shape the engine reports, as readable text.

    Objects are ``{key: shape}`` (``key?`` when it may be missing), arrays ``[shape]``,
    maps ``{[key]: shape}``, and a value that can be ``null`` ends in ``| null``.

    Example:
        >>> render('{"array":{"shape":{"object":[{"name":"id","value":{"shape":"integer","nullable":false},"optional":true}]},"nullable":true}}')
        '[{id?: integer} | null]'
    """
    return _render(json.loads(shape))


def _render(shape: typing.Any) -> str:
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


def _render_value(value: Mapping[str, typing.Any]) -> str:
    text = _render(value["shape"])
    return f"{text} | null" if value["nullable"] else text
