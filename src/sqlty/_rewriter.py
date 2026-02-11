"""SQL string rewriter — transforms between --sql strings and SQL[T](...) calls.

Provides two complementary operations:
- rewrite(): "--sql SELECT ..." → SQL[tuple[int]]("--sql\\nSELECT ...")
- strip():   SQL[tuple[int]]("--sql\\nSELECT ...") → "--sql\\nSELECT ..."

Both operations handle import injection/removal (datetime, uuid, SQL).

This module works at the source-code level (string → string), unlike
_decode.py which works at the codec level (bytes → string). The rewriter
is used by the CLI and mdtest runner; the codec is used by magic.py.
"""

from __future__ import annotations

import re

from sqlty._sql_analyzer import SQLAnalyzer
from sqlty._type_codec import TypeCodec


class SQLRewriter:
    """Rewrites Python source between --sql strings and SQL[T](...) calls.

    Args:
        dialect: SQL dialect for type inference (default: "postgres")
        schema_path: Optional path to schema file for schema-aware inference
        codec: Optional TypeCodec for custom type formatting
    """

    def __init__(
        self,
        dialect: str = "postgres",
        schema_path: str | None = None,
        codec: TypeCodec | None = None,
    ) -> None:
        from pathlib import Path

        self._analyzer = SQLAnalyzer(
            dialect=dialect,
            schema_path=Path(schema_path) if schema_path else None,
        )
        self._codec = codec

    def _infer_type(self, sql_content: str) -> str:
        """Infer the Python return type from a SQL query string."""
        query = sql_content
        if query.startswith("--sql "):
            query = query[6:].strip()
        elif query.startswith("--sql\\n"):
            # Source-text escape: \n (backslash + n) as it appears in source code
            query = query[7:].strip()
        elif query.startswith("--sql\n"):
            # Real newline (from triple-quoted strings)
            query = query[6:].strip()
        elif query.startswith("--sql"):
            query = query[5:].strip()

        try:
            result = self._analyzer.generate_return_type(query, codec=self._codec)
            return result if result else "Any"
        except Exception:
            return "Any"

    def rewrite(self, source: str) -> str:
        """Transform --sql prefixed strings into SQL[T](...) calls.

        Example:
            >>> r = SQLRewriter()
            >>> r.rewrite('q = "--sql SELECT id::INTEGER FROM users"')
            'from sqlty import SQL\\nq = SQL[tuple[int]]("--sql\\\\nSELECT id::INTEGER FROM users")'
        """
        lines = source.splitlines(keepends=True)
        result_lines: list[str] = []
        needs_sql_import = False
        needs_datetime = False
        needs_uuid = False
        has_sql_import = _has_import(source, "from sqlty import SQL")
        has_datetime_import = _has_import(source, "import datetime")
        has_uuid_import = _has_import(source, "import uuid")

        for line in lines:
            new_line = self._rewrite_line(line)
            if new_line != line:
                if "SQL[" in new_line:
                    needs_sql_import = True
                if "datetime." in new_line:
                    needs_datetime = True
                if "uuid." in new_line:
                    needs_uuid = True
            result_lines.append(new_line)

        result = "".join(result_lines)

        # Add needed imports
        imports_to_add: list[str] = []
        if needs_datetime and not has_datetime_import:
            imports_to_add.append("import datetime")
        if needs_uuid and not has_uuid_import:
            imports_to_add.append("import uuid")
        if needs_sql_import and not has_sql_import:
            imports_to_add.append("from sqlty import SQL")

        if imports_to_add:
            result = "\n".join(imports_to_add) + "\n" + result

        return result

    def _rewrite_line(self, line: str) -> str:
        """Rewrite a single line, replacing --sql strings with SQL[T](...)."""
        # Skip lines that already have SQL[...](...) wrappers
        if _SQL_WRAPPER_PATTERN.search(line):
            return line

        # Match: variable = "..." or variable = '...' or variable = """..."""
        # where the string contains --sql
        for pattern in _SQL_STRING_PATTERNS:
            match = pattern.search(line)
            if match:
                string_literal = match.group("str")
                content = _extract_string_content(string_literal)
                if content is not None and content.strip().startswith("--sql"):
                    inferred = self._infer_type(content.strip())
                    # Normalize: --sql<space> -> --sql\n in the string literal
                    normalized = _normalize_sql_prefix(string_literal)
                    replacement = f"SQL[{inferred}]({normalized})"
                    return line[: match.start("str")] + replacement + line[match.end("str") :]
        return line

    def strip(self, source: str) -> str:
        """Remove SQL[T](...) wrappers, restoring plain --sql strings.

        Example:
            >>> r = SQLRewriter()
            >>> r.strip('from sqlty import SQL\\nq = SQL[tuple[int]]("--sql\\\\nSELECT 1::INTEGER")')
            'q = "--sql\\\\nSELECT 1::INTEGER"'
        """
        lines = source.splitlines(keepends=True)
        result_lines: list[str] = []
        sql_wrapper_removed = False

        for line in lines:
            new_line = self._strip_line(line)
            if new_line != line:
                sql_wrapper_removed = True
            result_lines.append(new_line)

        result = "".join(result_lines)

        # Remove imports that are no longer needed
        if sql_wrapper_removed:
            result = self._remove_unused_imports(result)

        return result

    def _strip_line(self, line: str) -> str:
        """Strip SQL[T](...) from a single line, returning the inner string.

        Only strips auto-generated wrappers where the inner string starts
        with ``--sql`` followed by whitespace (space or newline). Manually
        written ``SQL[T]("my custom query")`` is left untouched.
        """
        match = _SQL_WRAPPER_PATTERN.search(line)
        if match:
            inner_string = match.group("inner")
            content = _extract_string_content(inner_string)
            # Only strip if the inner content starts with --sql + whitespace
            if content is not None and _is_sqlty_managed(content):
                return line[: match.start("wrapper")] + inner_string + line[match.end("wrapper") :]
        return line

    @staticmethod
    def _remove_unused_imports(source: str) -> str:
        """Remove sqlty/datetime/uuid imports if no longer referenced in source."""
        lines = source.splitlines(keepends=True)
        result: list[str] = []
        body = source  # full source for reference checking

        for line in lines:
            stripped = line.strip()

            # Remove "from sqlty import SQL" if SQL[ no longer appears
            if stripped == "from sqlty import SQL" and "SQL[" not in body:
                continue

            # Remove "import datetime" if datetime. no longer appears
            if stripped == "import datetime" and "datetime." not in body:
                continue

            # Remove "import uuid" if uuid. no longer appears
            if stripped == "import uuid" and "uuid." not in body:
                continue

            result.append(line)

        return "".join(result)


# --- Regex patterns ---

# Match string literals that might contain --sql
# Handles: "...", '...', """...""", '''...'''
_SQL_STRING_PATTERNS = [
    # Triple-quoted strings first (greedy)
    re.compile(r'(?P<str>""".*?""")', re.DOTALL),
    re.compile(r"(?P<str>'''.*?''')", re.DOTALL),
    # Single-line strings
    re.compile(r'(?P<str>"[^"]*")'),
    re.compile(r"(?P<str>'[^']*')"),
]

# Match SQL[T]("...") or SQL[T]('...') or SQL[T]("""...""")
_SQL_WRAPPER_PATTERN = re.compile(
    r'(?P<wrapper>SQL\[.*?\]\((?P<inner>""".*?"""|\'\'\'.*?\'\'\'|"[^"]*"|\'[^\']*\')\))',
    re.DOTALL,
)


def _extract_string_content(string_literal: str) -> str | None:
    """Extract the content from a quoted string literal."""
    if string_literal.startswith('"""') or string_literal.startswith("'''"):
        return string_literal[3:-3]
    if string_literal.startswith('"') or string_literal.startswith("'"):
        return string_literal[1:-1]
    return None


def _is_sqlty_managed(content: str) -> bool:
    """Check if string content is sqlty-managed (starts with --sql + whitespace).

    Returns True for strings that were auto-generated by ``sqlty rewrite``:
    - ``--sql\\nSELECT ...`` (--sql followed by source-text \\n escape)
    - ``--sql SELECT ...``  (--sql followed by space, legacy format)
    - ``--sql<newline>SELECT ...`` (--sql followed by real newline, triple-quoted)

    Returns False for manually written SQL wrappers like:
    - ``SELECT id FROM users``
    - ``my custom query``
    """
    stripped = content.lstrip()
    if not stripped.startswith("--sql"):
        return False
    rest = stripped[5:]
    if len(rest) == 0:
        return True
    # Source-text \n escape (backslash + n)
    if rest.startswith("\\n"):
        return True
    # Whitespace: space, real newline, tab
    return rest[0] in (" ", "\n", "\r", "\t")


def _normalize_sql_prefix(string_literal: str) -> str:
    """Normalize --sql<space> to --sql\\n in a string literal.

    Ensures the canonical format has --sql followed by a newline escape,
    not a space. Already-normalized strings (with \\n) are unchanged.
    """
    return string_literal.replace("--sql ", "--sql\\n", 1)


def _has_import(source: str, import_line: str) -> bool:
    """Check if source already contains a specific import."""
    return any(line.strip() == import_line for line in source.splitlines())
