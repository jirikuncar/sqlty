"""Decode utilities for --sql string transformation.

This module provides the core decode functionality without automatic codec registration.
Import from this module when you want to use the decode function without triggering
automatic codec registration.

Direct Usage (No Side Effects):
    from sqlty._decode import decode

    code = b'q = "--sql SELECT 1::INTEGER"'
    transformed, _ = decode(code)
    # transformed: 'from sqlty import SQL\\nq = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'

The decode function automatically:
    - Transforms strings prefixed with '--sql' into SQL[T](...) calls
    - Infers return types from SQL type casts (::INTEGER, ::TEXT, etc.)
    - Adds necessary imports (datetime, uuid, SQL) when needed
    - Uses isort for proper import ordering

Examples:
    Simple query:
        b'q = "--sql SELECT 1::INTEGER"'
        → 'from sqlty import SQL\\nq = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'

    Multiple columns:
        b'q = "--sql SELECT id::INTEGER, name::TEXT"'
        → 'from sqlty import SQL\\nq = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT")'

    With datetime/uuid:
        b'q = "--sql SELECT created_at::TIMESTAMP, user_id::UUID"'
        → 'import datetime\\nimport uuid\\nfrom sqlty import SQL\\n...'

Note: This module does NOT register any codecs. For automatic transformation
      via Python's codec system, use 'sqlty.magic' instead.
"""

from __future__ import annotations

import encodings
import io
import token
import tokenize

from sqlty._sql_analyzer import SQLAnalyzer

__all__ = ["decode", "_infer_sql_type", "_transform_sql_string"]

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
        needs_sql = False
        has_sql_import = False

        for tok in tokens:
            # Check if SQL is already imported
            if tok.type == token.NAME and tok.string == "SQL":
                # Look ahead to see if this is part of an import statement
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
                    if "SQL[" in new_string:
                        needs_sql = True

                    # Create a new token with the transformed string
                    new_tok = tokenize.TokenInfo(tok.type, new_string, tok.start, tok.end, tok.line)
                    new_tokens.append(new_tok)
                else:
                    new_tokens.append(tok)
            else:
                new_tokens.append(tok)

        # Reconstruct the source from tokens
        result = tokenize.untokenize(new_tokens)

        # Add necessary imports using isort for proper placement
        if needs_datetime or needs_uuid or (needs_sql and not has_sql_import):
            try:
                from isort import code as isort_code  # type: ignore[import-not-found]

                # Build the list of imports to add
                add_imports = []
                if needs_datetime:
                    add_imports.append("datetime")
                if needs_uuid:
                    add_imports.append("uuid")

                # Use isort to add imports in the correct location
                result = isort_code(
                    result,
                    add_imports=add_imports,
                    force_single_line=False,
                    line_length=100,
                )

                # Add SQL import separately since it's a from import
                if needs_sql and not has_sql_import:
                    # Add the SQL import at the beginning (isort will position it correctly)
                    result = "from sqlty import SQL\n" + result
                    # Run through isort again to properly position it
                    result = isort_code(result, force_single_line=False, line_length=100)
            except ImportError:
                # Fallback to simple prepending if isort is not available
                imports = []
                if needs_datetime:
                    imports.append("import datetime")
                if needs_uuid:
                    imports.append("import uuid")
                if needs_sql and not has_sql_import:
                    imports.append("from sqlty import SQL")
                if imports:
                    import_lines = "\n".join(imports) + "\n"
                    result = import_lines + result

        return result, length

    except Exception:
        # If anything goes wrong, return the original
        return u, length
