r"""The ``sqlty`` codec's decoder, without registering the codec.

``decode`` turns source bytes into text with ``--sql`` strings wrapped as
``SQL[T](...)``, as ``sqlty rewrite`` does, and adds the imports they need:

    >>> from sqlty._decode import decode
    >>> decode(b'q = "--sql\\nSELECT 1::INTEGER"')[0]
    'from sqlty import SQL; q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")'

Strings whose row type can't be inferred (invalid SQL, DML without
RETURNING, DDL, empty queries) are left as they are. The imports are joined
with ``;`` onto an existing line (after the docstring or ``__future__``
imports, else before the first statement) rather than a new line above
the code: when CPython runs a file as a script, it decodes from the end of the
coding line and drops the first decoded line (#111). Line numbers match the file
on disk, except when a compound statement (``def``, ``class``, ``@``, ``if``,
...) comes right after the coding line, with no docstring, blank or comment
line in between; then one line is added. The ``# coding: sqlty``
declaration becomes ``utf-8``, so the decoded text runs without the codec.
``import-style`` comes from ``[tool.sqlty]`` in pyproject.toml; on any error the
source is returned unchanged. Import ``sqlty.magic`` to register the codec.
"""

from __future__ import annotations

import codecs
import encodings
import re
from collections.abc import Buffer
from functools import cache

__all__ = ["decode"]

_utf_8_codec = encodings.search_function("utf8")
assert _utf_8_codec is not None, "UTF-8 codec not found"
utf_8_codec: codecs.CodecInfo = _utf_8_codec

# PEP 263 declaration of this codec on the first or second line.
_CODING = re.compile(r"\A((?:[^\n]*\n)?[ \t\f]*#[^\n]*?coding[:=][ \t]*)sqlty\b")


@cache
def _import_style() -> str | None:
    try:
        from sqlty._config import load_config

        config, _ = load_config()
        return config.get("import_style")
    except Exception:
        return None


def decode(b: Buffer, errors: str = "strict") -> tuple[str, int]:
    """Decode UTF-8 ``b``, wrapping ``--sql`` strings; returns ``(text, length)``."""
    source, length = utf_8_codec.decode(b, errors)
    try:
        from sqlty.core import rewrite_source

        decoded = rewrite_source(source, import_style=_import_style(), keep_lines=True)
        return _CODING.sub(r"\1utf-8", decoded, count=1), length
    except Exception:
        return source, length
