"""Custom codec for --sql string prefix transformation.

This module provides utilities to transform string literals prefixed with '--sql'
into SQL[T](...) calls with type inference.

Due to Python's codec caching, automatic transformation requires setup via
sitecustomize.py or similar mechanism. See docs/magic-codec.md for details.

Direct Usage:
    from sqlty.magic import decode

    code = b'q = "--sql SELECT 1::INTEGER"'
    transformed, _ = decode(code)
    # transformed: 'q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'

For Automatic Transformation:
    Create sitecustomize.py in your site-packages:
        import sqlty.magic

    Then in your code (no encoding declaration needed):
        q = "--sql SELECT 1::INTEGER"
        # Automatically transforms to: SQL[tuple[int]]("--sql SELECT 1::INTEGER")
"""

from __future__ import annotations

import codecs
import encodings
import io
import token
import tokenize
from typing import TYPE_CHECKING, Any

from sqlty._sql_analyzer import SQLAnalyzer

if TYPE_CHECKING:
    from codecs import CodecInfo

# Get the standard UTF-8 codec
utf_8_codec = encodings.search_function("utf8")
assert utf_8_codec is not None, "UTF-8 codec not found"


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

    if string_literal.startswith('"""') or string_literal.startswith("'''"):
        content = string_literal[3:-3]
    elif string_literal.startswith('"') or string_literal.startswith("'"):
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
    u, length = utf_8_codec.decode(b, errors)  # type: ignore[union-attr]

    try:
        # Tokenize the source
        tokens = list(tokenize.generate_tokens(io.StringIO(u).readline))

        # Track which tokens need transformation and which imports are needed
        new_tokens = []
        needs_datetime = False
        needs_uuid = False

        for tok in tokens:
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

        # Add necessary imports at the top if needed
        imports = []
        if needs_datetime:
            imports.append("import datetime")
        if needs_uuid:
            imports.append("import uuid")

        if imports:
            # Add imports at the beginning
            import_lines = "\n".join(imports) + "\n"
            result = import_lines + result

        return result, length

    except Exception:
        # If anything goes wrong, return the original
        return u, length


class IncrementalDecoder(codecs.BufferedIncrementalDecoder):
    """Incremental decoder for the sqlty-magic codec."""

    def _buffer_decode(  # type: ignore[override]
        self, input: bytes, errors: str, final: bool
    ) -> tuple[str, int]:  # pragma: no cover
        if final:
            return decode(input, errors)
        else:
            return "", 0


class StreamReader(utf_8_codec.streamreader):  # type: ignore[name-defined,misc]
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


# Create the custom codec info that will be used for UTF-8 and sqlty
_custom_codec_info = codecs.CodecInfo(
    name="utf-8",
    encode=utf_8_codec.encode,
    decode=decode,  # type: ignore[arg-type]
    incrementalencoder=utf_8_codec.incrementalencoder,
    incrementaldecoder=IncrementalDecoder,
    streamreader=StreamReader,
    streamwriter=utf_8_codec.streamwriter,
)

# Also create a separate codec info for "sqlty" for testing purposes
_sqlty_codec_info = codecs.CodecInfo(
    name="sqlty",
    encode=utf_8_codec.encode,
    decode=decode,  # type: ignore[arg-type]
    incrementalencoder=utf_8_codec.incrementalencoder,
    incrementaldecoder=IncrementalDecoder,
    streamreader=StreamReader,
    streamwriter=utf_8_codec.streamwriter,
)


def _codec_search(encoding: str) -> CodecInfo | None:
    """Search function for codec registration.

    This function provides the custom codec for:
    - UTF-8 and its aliases (for automatic transformation)
    - "sqlty" (for explicit testing)

    Args:
        encoding: The encoding name being looked up

    Returns:
        Custom CodecInfo for supported encodings, None otherwise
    """
    # Normalize the encoding name (lowercase, remove hyphens and underscores)
    normalized = encoding.lower().replace("-", "").replace("_", "")

    # Handle UTF-8 and its common aliases
    if normalized in ("utf8", "utf", "u8"):
        return _custom_codec_info

    # Handle "sqlty" for explicit testing
    if normalized == "sqlty":
        return _sqlty_codec_info

    return None


def register() -> None:
    """Register the custom codec with Python's codec registry.

    This attempts to override UTF-8 codec for automatic --sql string transformation.

    Note: Due to Python's codec caching (built-in codecs are registered before user
    code runs), this only works when called very early, ideally from sitecustomize.py.

    For reliable automatic transformation, create a sitecustomize.py file in your
    site-packages directory containing:
        import sqlty.magic

    Alternatively, use the decode() function directly to transform source code.
    """
    import _codecs

    # Register our search function
    codecs.register(_codec_search)

    # Attempt to clear the codec cache
    # This may not work if UTF-8 was already cached before this runs
    try:
        _codecs.unregister(_codec_search)
        codecs.register(_codec_search)
    except Exception:
        # If unregister fails, just continue with registration
        pass


# Automatically register when the module is imported
# This works best when imported from sitecustomize.py
register()
