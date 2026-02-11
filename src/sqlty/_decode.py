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
from functools import lru_cache
from pathlib import Path
from typing import cast

from sqlty._config import ReturnFormat
from sqlty._sql_analyzer import SQLAnalyzer

__all__ = ["decode", "_infer_sql_type", "_transform_sql_string"]

# Get the standard UTF-8 codec
utf_8_codec = encodings.search_function("utf8")
assert utf_8_codec is not None, "UTF-8 codec not found"


@lru_cache(maxsize=8)
def _load_return_format(cwd_path: str) -> str:
    """Load return_format from config with caching.
    
    Args:
        cwd_path: Current working directory as a string (for hashability)
    
    Returns:
        Return format string
    """
    try:
        from sqlty._config import load_config
        
        config, _ = load_config(Path(cwd_path))
        if config:
            return config.get("return_format", "tuple")
    except Exception:
        pass
    return "tuple"


def _extract_return_format_from_coding(source: str) -> str | None:
    """Extract return format from coding declaration.
    
    Parses coding declarations like:
    - # coding: sqlty?return-format=typeddict
    - # -*- coding: sqlty?return-format=namedtuple -*-
    
    Args:
        source: The source code to parse
    
    Returns:
        Return format if found in coding declaration, None otherwise
    """
    import re
    
    # Check first two lines for coding declaration
    lines = source.split('\n', 2)[:2]
    
    for line in lines:
        # Match coding declarations with query parameters
        # Supports: # coding: sqlty?return-format=xxx
        #           # -*- coding: sqlty?return-format=xxx -*-
        match = re.search(r'coding[=:]\s*sqlty\?(.+?)(?:\s|$|-\*-)', line)
        if match:
            params_str = match.group(1)
            # Parse query parameters
            for param in params_str.split('&'):
                if '=' in param:
                    key, value = param.split('=', 1)
                    if key in ('return-format', 'return_format'):
                        return value.strip()
    
    return None


def _infer_sql_type(sql_query: str, return_format: str = "tuple") -> str:
    """Infer the return type from a SQL query.

    Args:
        sql_query: The SQL query string (may include --sql prefix)
        return_format: Format for the return type - "tuple", "typeddict", "namedtuple", or "pydantic"

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
        
        # Generate a class name for non-tuple formats
        class_name = "QueryRow"
        if return_format != "tuple":
            import hashlib
            query_hash = hashlib.md5(query.encode()).hexdigest()[:8]
            class_name = f"QueryRow_{query_hash}"
        
        inferred_type = analyzer.generate_return_type(
            query, return_format=return_format, class_name=class_name
        )
        if inferred_type:
            return inferred_type
        return "Any"
    except Exception:
        # If type inference fails, default to Any
        return "Any"


def _transform_sql_string(string_literal: str, return_format: str = "tuple") -> tuple[str, str | None, str]:
    """Transform a --sql prefixed string into SQL[T](...) call.

    Args:
        string_literal: The complete string literal including quotes
        return_format: Format for the return type - "tuple", "typeddict", "namedtuple", or "pydantic"

    Returns:
        A tuple of (transformed string, SQL query content or None, class name used)
        The SQL query content is returned for class definition generation
        The class name is what was actually used in the transformation
    """
    # Parse the string literal to extract the actual content
    # Handle both single and double quotes, and triple quotes

    if string_literal.startswith('"""') or string_literal.startswith("'''"):
        content = string_literal[3:-3]
    elif string_literal.startswith('"') or string_literal.startswith("'"):
        content = string_literal[1:-1]
    else:
        # Not a valid string literal
        return string_literal, None, ""

    # Check if content starts with --sql
    stripped_content = content.strip()
    if not stripped_content.startswith("--sql"):
        return string_literal, None, ""

    # Infer the type from the SQL query (generates QueryRow_xxx for non-tuple formats)
    inferred_type = _infer_sql_type(stripped_content, return_format=return_format)

    # Construct the SQL[T](...) call
    transformed = f"SQL[{inferred_type}]({string_literal})"

    # Return the transformed string, query content, and the class name used
    return transformed, stripped_content, inferred_type


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

    # Try to extract return format from coding declaration first
    coding_format = _extract_return_format_from_coding(u)
    
    # Use coding declaration format if specified, otherwise load from config
    if coding_format:
        return_format = coding_format
    else:
        return_format = _load_return_format(str(Path.cwd()))

    # Track which imports are needed
    class ImportFlags:
        """Track which imports are needed."""
        datetime: bool = False
        uuid: bool = False
        sql: bool = False
        has_sql_import: bool = False
        protocol: bool = False  # Protocol is needed for all non-tuple formats

    flags = ImportFlags()

    # Track SQL queries for class definition generation
    sql_queries: list[str] = []

    try:
        # Tokenize the source
        tokens = list(tokenize.generate_tokens(io.StringIO(u).readline))

        # Track which tokens need transformation
        new_tokens = []

        for i, tok in enumerate(tokens):
            # Check if SQL is already imported
            if tok.type == token.NAME and tok.string == "SQL":
                # Look ahead to see if this is part of an import statement
                flags.has_sql_import = True

            if tok.type == token.STRING:
                # Check if this is a --sql string
                if "--sql" in tok.string:
                    # Transform the token - the type name will be inferred from the query
                    new_string, query_content, class_name = _transform_sql_string(
                        tok.string, return_format=return_format
                    )

                    # Collect SQL query and its class name for class definition generation
                    if query_content and return_format != "tuple":
                        sql_queries.append((query_content, class_name))

                    # Check if the transformation needs Protocol import
                    match return_format:
                        case "typeddict" | "namedtuple" | "pydantic":
                            flags.protocol = True  # All non-tuple formats need Protocol
                        case "tuple":
                            pass  # No additional imports needed
                    
                    # Check for type-specific imports
                    if "datetime." in new_string:
                        flags.datetime = True
                    if "uuid." in new_string:
                        flags.uuid = True
                    if "SQL[" in new_string:
                        flags.sql = True

                    # Create a new token with the transformed string
                    new_tok = tokenize.TokenInfo(tok.type, new_string, tok.start, tok.end, tok.line)
                    new_tokens.append(new_tok)
                else:
                    new_tokens.append(tok)
            else:
                new_tokens.append(tok)

        # Reconstruct the source from tokens
        result_bytes = tokenize.untokenize(new_tokens)
        # Decode bytes to string for further processing
        if isinstance(result_bytes, bytes):
            result: str = result_bytes.decode("utf-8")
        else:
            result = str(result_bytes)

        # Generate class definitions for types
        # First, check which types are already defined in the source
        existing_types: set[str] = set()
        for i, tok in enumerate(tokens):
            if tok.type == token.NAME and i + 1 < len(tokens):
                # Check if this name is being defined (followed by '=')
                next_tok = tokens[i + 1]
                if next_tok.type == token.OP and next_tok.string == '=':
                    existing_types.add(tok.string)

        class_definitions: list[str] = []
        if sql_queries and return_format != "tuple":
            analyzer = SQLAnalyzer()
            seen_classes: set[str] = set()

            for query_content, class_name in sql_queries:
                # Skip if already processed
                if class_name in seen_classes:
                    continue
                seen_classes.add(class_name)

                # Extract the query without --sql prefix
                clean_query = query_content
                if clean_query.startswith("--sql "):
                    clean_query = clean_query[6:].strip()
                elif clean_query.startswith("--sql"):
                    clean_query = clean_query[5:].strip()

                # Only generate if not already defined in source
                if class_name not in existing_types:
                    # Generate class definition
                    class_def = analyzer.generate_class_definition(
                        clean_query, return_format=cast(ReturnFormat, return_format), class_name=class_name
                    )
                    if class_def:
                        class_definitions.append(class_def)

        # Add necessary imports using isort for proper placement
        if (
            flags.datetime
            or flags.uuid
            or (flags.sql and not flags.has_sql_import)
            or flags.protocol
        ):
            try:
                from isort import code as isort_code  # type: ignore[import-not-found]

                # Build the list of imports to add
                add_imports = []
                if flags.datetime:
                    add_imports.append("datetime")
                if flags.uuid:
                    add_imports.append("uuid")

                # Use isort to add imports in the correct location
                result = isort_code(
                    result,
                    add_imports=add_imports,
                    force_single_line=False,
                    line_length=100,
                )

                # Add SQL import separately since it's a from import
                if flags.sql and not flags.has_sql_import:
                    # Add the SQL import at the beginning (isort will position it correctly)
                    result = "from sqlty import SQL\n" + result
                    # Run through isort again to properly position it
                    result = isort_code(result, force_single_line=False, line_length=100)

                # Add Protocol and runtime_checkable imports for non-tuple formats
                if flags.protocol:
                    result = "from typing import Protocol, runtime_checkable\n" + result
                    result = isort_code(result, force_single_line=False, line_length=100)

                # Add class definitions after imports
                if class_definitions:
                    # Find where to insert class definitions (after imports, before first non-import line)
                    # For simplicity, add them after all imports with a blank line
                    class_defs_str = "\n\n" + "\n\n".join(class_definitions) + "\n"
                    # Find the end of imports by looking for the first line that's not an import or blank
                    lines = result.split("\n")
                    insert_pos = 0
                    for i, line in enumerate(lines):
                        stripped = line.strip()
                        if stripped and not stripped.startswith(("import ", "from ", "#")):
                            insert_pos = i
                            break
                    else:
                        insert_pos = len(lines)

                    # Insert class definitions
                    lines.insert(insert_pos, class_defs_str)
                    result = "\n".join(lines)
            except ImportError:
                # Fallback to simple prepending if isort is not available
                imports = []
                if flags.datetime:
                    imports.append("import datetime")
                if flags.uuid:
                    imports.append("import uuid")
                if flags.sql and not flags.has_sql_import:
                    imports.append("from sqlty import SQL")
                if flags.protocol:
                    imports.append("from typing import Protocol, runtime_checkable")
                if imports:
                    import_lines = "\n".join(imports) + "\n"
                    result = import_lines + result

                # Add class definitions after imports (fallback case)
                if class_definitions:
                    class_defs_str = "\n\n" + "\n\n".join(class_definitions) + "\n"
                    # Find the end of imports
                    lines = result.split("\n")
                    insert_pos = 0
                    for i, line in enumerate(lines):
                        stripped = line.strip()
                        if stripped and not stripped.startswith(("import ", "from ", "#")):
                            insert_pos = i
                            break
                    else:
                        insert_pos = len(lines)

                    # Insert class definitions
                    lines.insert(insert_pos, class_defs_str)
                    result = "\n".join(lines)

        return result, length

    except Exception:
        # If anything goes wrong, return the original
        return u, length
