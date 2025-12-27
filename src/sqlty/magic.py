"""Custom codec for --sql string prefix transformation.

This module provides a custom encoding codec that automatically transforms
string literals prefixed with '--sql' into SQL[T](...) calls with type inference.

Usage:
    # At the top of your file (before any imports)
    # coding: sqlty-magic

    from sqlty import magic

    # This string will be automatically transformed
    q = "--sql SELECT 1"
    # Becomes: q = SQL[int]("--sql SELECT 1")
"""

from __future__ import annotations

import codecs
import encodings
import io
import token
import tokenize
from typing import Any

from sqlty._sql_analyzer import SQLAnalyzer

# Get the standard UTF-8 codec
utf_8 = encodings.search_function("utf8")


def _infer_sql_type(sql_query: str) -> str:
    """Infer the return type from a SQL query.

    Args:
        sql_query: The SQL query string (may include --sql prefix)

    Returns:
        A string representation of the inferred type
    """
    # Remove the --sql prefix if present
    query = sql_query
    if query.startswith("--sql "):
        query = query[6:].strip()
    elif query.startswith("--sql"):
        query = query[5:].strip()

    try:
        # Use the existing SQL analyzer to infer types
        analyzer = SQLAnalyzer()
        inferred_type = analyzer.generate_return_type(query)
        if inferred_type:
            return inferred_type
        return "Any"
    except Exception:
        # If type inference fails, default to Any
        return "Any"


def _transform_sql_string(string_literal: str) -> str:
    """Transform a --sql prefixed string into SQL[T](...) call.

    Args:
        string_literal: The complete string literal including quotes

    Returns:
        The transformed string as a SQL[T](...) call
    """
    # Parse the string literal to extract the actual content
    # Handle both single and double quotes, and triple quotes
    quote_char = None
    is_triple = False

    if string_literal.startswith('"""') or string_literal.startswith("'''"):
        is_triple = True
        quote_char = string_literal[:3]
        content = string_literal[3:-3]
    elif string_literal.startswith('"') or string_literal.startswith("'"):
        quote_char = string_literal[0]
        content = string_literal[1:-1]
    else:
        # Not a valid string literal
        return string_literal

    # Check if content starts with --sql
    stripped_content = content.strip()
    if not stripped_content.startswith("--sql"):
        return string_literal

    # Infer the type from the SQL query (use stripped content for inference)
    inferred_type = _infer_sql_type(stripped_content)

    # Construct the SQL[T](...) call
    return f"SQL[{inferred_type}]({string_literal})"


def decode(b: bytes, errors: str = "strict") -> tuple[str, int]:
    """Decode bytes into a string, transforming --sql prefixed strings.

    This is the main entry point for the custom codec.

    Args:
        b: The bytes to decode
        errors: Error handling scheme

    Returns:
        A tuple of (decoded_string, length)
    """
    # First decode as UTF-8
    u, length = utf_8.decode(b, errors)

    try:
        # Tokenize the source
        tokens = list(tokenize.generate_tokens(io.StringIO(u).readline))

        # Track which tokens need transformation
        new_tokens = []
        for tok in tokens:
            if tok.type == token.STRING:
                # Check if this is a --sql string
                if "--sql" in tok.string:
                    # Transform the token
                    new_string = _transform_sql_string(tok.string)
                    # Create a new token with the transformed string
                    new_tok = tokenize.TokenInfo(
                        tok.type, new_string, tok.start, tok.end, tok.line
                    )
                    new_tokens.append(new_tok)
                else:
                    new_tokens.append(tok)
            else:
                new_tokens.append(tok)

        # Reconstruct the source from tokens
        result = tokenize.untokenize(new_tokens)
        return result, length

    except Exception:
        # If anything goes wrong, return the original
        return u, length


class IncrementalDecoder(codecs.BufferedIncrementalDecoder):
    """Incremental decoder for the sqlty-magic codec."""

    def _buffer_decode(
        self, input: bytes, errors: str, final: bool
    ) -> tuple[str, int]:  # pragma: no cover
        if final:
            return decode(input, errors)
        else:
            return "", 0


class StreamReader(utf_8.streamreader, object):
    """Stream reader for the sqlty-magic codec.

    Decode is deferred to support better error messages.
    """

    _stream: Any = None
    _decoded: bool = False

    @property
    def stream(self) -> Any:
        """Get the decoded stream."""
        if not self._decoded:
            text, _ = decode(self._stream.read())
            self._stream = io.BytesIO(text.encode("UTF-8"))
            self._decoded = True
        return self._stream

    @stream.setter
    def stream(self, stream: Any) -> None:
        """Set the stream to decode."""
        self._stream = stream
        self._decoded = False


# Codec map for registration
codec_map = {
    name: codecs.CodecInfo(
        name=name,
        encode=utf_8.encode,
        decode=decode,
        incrementalencoder=utf_8.incrementalencoder,
        incrementaldecoder=IncrementalDecoder,
        streamreader=StreamReader,
        streamwriter=utf_8.streamwriter,
    )
    for name in ("sqlty-magic", "sqlty_magic")
}


def register() -> None:
    """Register the sqlty-magic codec with Python's codec registry.

    This function should be called before importing any modules that use
    the --sql string prefix syntax.
    """
    codecs.register(codec_map.get)


# Automatically register when the module is imported
register()
