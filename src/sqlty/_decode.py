"""Decode utilities for --sql string transformation.

This module provides the core decode functionality without automatic codec registration.
Import from this module when you want to use the decode function without triggering
automatic codec registration.

Direct Usage (No Side Effects):
    from sqlty._decode import decode

    code = b'q = "--sql SELECT 1::INTEGER"'
    transformed, _ = decode(code)
    # transformed: 'from sqlty import SQL; q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'

The decode function automatically:
    - Transforms strings prefixed with '--sql' into SQL[T](...) calls
    - Infers return types from SQL type casts (::INTEGER, ::TEXT, etc.)
    - Adds necessary imports (datetime, uuid, SQL) when needed, on an
      existing line so line numbers match the file on disk
    - Leaves strings unchanged when inference fails (invalid SQL, DML
      without RETURNING, DDL, empty queries)
    - Never modifies string content — only adds/removes the SQL[T]() wrapper

Examples:
    Simple query:
        b'q = "--sql SELECT 1::INTEGER"'
        → 'from sqlty import SQL; q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'

    Multiple columns:
        b'q = "--sql SELECT id::INTEGER, name::TEXT"'
        → 'from sqlty import SQL; q = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT")'

    Invalid/non-result queries are left unchanged:
        b'q = "--sql UPDATE users SET name = 1"'
        → unchanged (no type to infer)

Note: This module does NOT register any codecs. For automatic transformation
      via Python's codec system, use 'sqlty.magic' instead.
"""

from __future__ import annotations

import codecs
import encodings
import io
import token
import tokenize
from collections.abc import Buffer
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from sqlty._rewriter import ImportStyle, SQLRewriter

__all__ = ["decode", "_infer_sql_type", "_transform_sql_string"]

# Get the standard UTF-8 codec
_utf_8_codec = encodings.search_function("utf8")
assert _utf_8_codec is not None, "UTF-8 codec not found"
utf_8_codec: codecs.CodecInfo = _utf_8_codec

# Lazy-initialized rewriter (avoid import cycle at module level)
_rewriter = None
_import_style_str: str | None = None


def _get_rewriter() -> SQLRewriter:
    """Get or create the module-level SQLRewriter instance.

    Loads ``import-style`` from pyproject.toml so the codec emits the
    same wrapper prefix and import line as the CLI commands.
    """
    global _rewriter, _import_style_str
    if _rewriter is None:
        from sqlty._rewriter import SQLRewriter

        # Load import-style from pyproject.toml
        try:
            from sqlty._config import load_config

            config, _ = load_config()
            _import_style_str = config.get("import_style")
        except Exception:
            _import_style_str = None

        _rewriter = SQLRewriter(import_style=_import_style_str)
    return _rewriter


def _get_import_style() -> ImportStyle:
    """Return the parsed ImportStyle for import injection."""
    from sqlty._rewriter import ImportStyle

    _get_rewriter()  # ensure config loaded
    return ImportStyle.parse(_import_style_str) if _import_style_str else ImportStyle.default()


def _infer_sql_type(sql_query: str) -> str:
    """Infer the return type from a SQL query.

    Args:
        sql_query: The SQL query string (may include --sql prefix)

    Returns:
        A string representation of the inferred type
    """
    return _get_rewriter()._infer_type(sql_query)


def _transform_sql_string(string_literal: str) -> str:
    """Transform a --sql prefixed string into SQL[T](...) call.

    Only wraps when type inference succeeds with a concrete type.
    Returns the original string unchanged for empty queries, invalid SQL,
    DML without RETURNING, DDL, and other non-result-set statements.

    Args:
        string_literal: The complete string literal including quotes

    Returns:
        The transformed string as a SQL[T](...) call, or the original if
        inference fails
    """
    from sqlty._rewriter import _extract_string_content

    content = _extract_string_content(string_literal)
    if content is None or not content.strip().startswith("--sql"):
        return string_literal

    inferred_type = _infer_sql_type(content.strip())
    # Don't wrap when inference fails — SQL[Any] is never useful.
    if inferred_type == "Any":
        return string_literal
    istyle = _get_import_style()
    return f"{istyle.wrapper_name}[{inferred_type}]({string_literal})"


_COMPOUND = frozenset({"async", "class", "def", "for", "if", "try", "while", "with"})


def _import_anchor(
    new_tokens: list[tokenize.TokenInfo], tokens: list[tokenize.TokenInfo]
) -> tuple[int, str]:
    """Return the token to prefix with the added imports and the separator to use.

    The imports go after the module docstring and ``from __future__`` imports,
    in front of the first other statement, without adding a line, so line
    numbers match the file on disk. Line 1 is never used: when Python runs a
    file as a script, it decodes the bytes after the coding cookie line and
    discards the first decoded line, which is the rest of the cookie line
    (``fp_setreadl`` in CPython's ``Parser/tokenizer/file_tokenizer.c``).
    A compound statement cannot follow ``;``, so the imports then take a blank
    or comment line above it, or a new line as a last resort.
    """
    prev_row = 1
    i = 0
    first_statement = True
    while True:
        j = i
        while tokens[j].type in (token.NL, token.COMMENT):
            j += 1
        k = j
        while tokens[k].type not in (token.NEWLINE, token.ENDMARKER):
            k += 1
        stmt = [t for t in tokens[j:k] if t.type != token.COMMENT]
        if not stmt:
            return j, "\n"
        docstring = first_statement and all(
            t.type == token.STRING and new_tokens[n].string == t.string
            for n, t in enumerate(tokens[j:k], j)
            if t.type != token.COMMENT
        )
        future = [t.string for t in stmt[:2]] == ["from", "__future__"]
        first_statement = False
        if docstring or future:
            prev_row = tokens[k].start[0]
            i = k + 1
            continue
        first = stmt[0]
        if not (
            (first.type == token.NAME and first.string in _COMPOUND)
            or first.string == "@"
            or stmt[-1].string == ":"
        ):
            return j, "; "
        row = first.start[0] - 1
        if row > prev_row:
            above = next(n for n in range(i, j) if tokens[n].start[0] == row)
            if tokens[above].start[1] == 0:
                return above, "" if tokens[above].type == token.NL else "  "
        return j, "\n"


def decode(b: Buffer, errors: str = "strict") -> tuple[str, int]:
    """Decode bytes into a string, transforming --sql prefixed strings.

    This is the main entry point for the custom codec. Uses the tokenizer
    for precise string detection, then delegates type inference to SQLRewriter.

    Args:
        b: The bytes to decode
        errors: Error handling scheme

    Returns:
        A tuple of (decoded_string, length)
    """
    # First decode as UTF-8
    u, length = utf_8_codec.decode(b, errors)

    try:
        # Tokenize the source
        tokens = list(tokenize.generate_tokens(io.StringIO(u).readline))

        # Track which tokens need transformation and which imports are needed
        new_tokens = []
        needs_datetime = False
        needs_uuid = False
        needs_sql = False
        has_sql_import = False

        istyle = _get_import_style()

        for tok in tokens:
            # Check if SQL is already imported
            if tok.type == token.NAME and tok.string == "SQL":
                has_sql_import = True

            if tok.type == token.STRING:
                # Check if this is a --sql string
                if "--sql" in tok.string:
                    # Transform the token
                    new_string = _transform_sql_string(tok.string)

                    # Check if the transformation needs imports
                    if "datetime." in new_string:
                        needs_datetime = True
                    if "uuid." in new_string:
                        needs_uuid = True
                    if f"{istyle.wrapper_name}[" in new_string:
                        needs_sql = True

                    # Create a new token with the transformed string
                    new_tok = tokenize.TokenInfo(tok.type, new_string, tok.start, tok.end, tok.line)
                    new_tokens.append(new_tok)
                else:
                    new_tokens.append(tok)
            else:
                new_tokens.append(tok)

        imports = []
        if needs_datetime:
            imports.append("import datetime")
        if needs_uuid:
            imports.append("import uuid")
        if needs_sql and not has_sql_import:
            imports.append(istyle.import_line)
        if imports:
            index, sep = _import_anchor(new_tokens, tokens)
            anchor = new_tokens[index]
            new_tokens[index] = anchor._replace(string="; ".join(imports) + sep + anchor.string)

        result = tokenize.untokenize(new_tokens)

        return result, length

    except Exception:
        # If anything goes wrong, return the original
        return u, length
