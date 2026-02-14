"""SQL string rewriter — transforms between --sql strings and SQL[T](...) calls.

Provides two complementary operations:
- rewrite(): "--sql SELECT ..." → SQL[tuple[int]]("--sql SELECT ...")
- strip():   SQL[tuple[int]]("--sql SELECT ...") → "--sql SELECT ..."

The rewriter never modifies the string content — only adds/removes the
SQL[T]() wrapper.  This guarantees strip(rewrite(source)) == source.

Both operations handle import injection/removal (datetime, uuid, SQL).

This module works at the source-code level (string → string), unlike
_decode.py which works at the codec level (bytes → string). The rewriter
is used by the CLI and mdtest runner; the codec is used by magic.py.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from sqlty._sql_analyzer import SQLAnalyzer
from sqlty._type_codec import TypeCodec


@dataclass
class RewriteResult:
    """Result of a rewrite operation, including any warnings."""

    source: str
    warnings: list[str] = field(default_factory=list)


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
        """Infer the Python return type from a SQL query string.

        Passes the content directly to the SQL parser, letting SQL comment
        semantics handle the ``--sql`` prefix naturally:

        - ``--sql\\nSELECT ...`` → ``\\n`` becomes real newline; parser sees
          ``--sql`` as comment and ``SELECT ...`` as the query.
        - ``--sql<newline>SELECT ...`` → triple-quoted; parser sees comment + query.
        - ``--sql SELECT ...``  → entire line is a SQL comment → empty query.
        """
        query = sql_content
        # Convert source-text \\n escape to real newline so the parser
        # sees --sql as a comment and the query on the next line.
        if query.startswith("--sql\\n"):
            query = query.replace("\\n", "\n", 1)

        try:
            result = self._analyzer.generate_return_type(query, codec=self._codec)
            return result if result else "Any"
        except Exception:
            return "Any"

    def rewrite(self, source: str) -> RewriteResult:
        """Transform --sql prefixed strings into SQL[T](...) calls.

        Uses whole-source matching so triple-quoted multiline strings are
        handled correctly.  Returns a RewriteResult containing the rewritten
        source and any warnings (e.g. skipped f/t-strings).

        Example:
            >>> r = SQLRewriter()
            >>> r.rewrite('q = "--sql SELECT id::INTEGER FROM users"').source
            'from sqlty import SQL\\nq = SQL[tuple[int]]("--sql\\\\nSELECT id::INTEGER FROM users")'
        """
        warnings: list[str] = []

        # --- Pass 1: detect f/t-strings (line-by-line, for warnings) ---
        fstring_lines: set[int] = set()
        for line_num, line in enumerate(source.splitlines(), start=1):
            if _FSTRING_SQL_PATTERN.search(line):
                fstring_lines.add(line_num)
                is_tstring = bool(re.search(r"""[tT]\s*["']""", line))
                kind = "t-string" if is_tstring else "f-string"
                rule = "tstring-skipped" if is_tstring else "fstring-skipped"
                warnings.append(
                    f"line {line_num}: {kind} with --sql prefix skipped "
                    f"({kind}s cannot be statically rewritten) [{rule}]"
                )

        # --- Pass 2: whole-source rewrite ---
        has_sql_import = _has_import(source, "from sqlty import SQL")
        has_datetime_import = _has_import(source, "import datetime")
        has_uuid_import = _has_import(source, "import uuid")
        needs_sql_import = False
        needs_datetime = False
        needs_uuid = False

        # Pre-compute existing SQL[T](...) wrapper spans to skip
        wrapper_spans = [
            (m.start(), m.end()) for m in _SQL_WRAPPER_PATTERN.finditer(source)
        ]

        # Collect all replacements, then apply right-to-left
        replacements: list[tuple[int, int, str]] = []
        for match in _SQL_STRING_PATTERN.finditer(source):
            start, end = match.start(), match.end()

            # Skip matches inside existing SQL[T](...) wrappers
            if any(ws <= start and end <= we for ws, we in wrapper_spans):
                continue

            # Skip matches on lines that contain f/t-strings
            if fstring_lines and _line_at(source, start) in fstring_lines:
                continue

            string_literal = match.group("str")
            content = _extract_string_content(string_literal)
            if content is None or not content.strip().startswith("--sql"):
                continue

            inferred = self._infer_type(content.strip())
            # Don't wrap when inference fails — SQL[Any] is never useful.
            if inferred == "Any":
                continue

            replacement = f"SQL[{inferred}]({string_literal})"
            replacements.append((match.start("str"), match.end("str"), replacement))

            needs_sql_import = True
            if "datetime." in replacement:
                needs_datetime = True
            if "uuid." in replacement:
                needs_uuid = True

        # Apply right-to-left to preserve earlier positions
        result = source
        for start, end, replacement in reversed(replacements):
            result = result[:start] + replacement + result[end:]

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

        return RewriteResult(source=result, warnings=warnings)

    def strip(self, source: str) -> str:
        """Remove SQL[T](...) wrappers, restoring plain --sql strings.

        Uses whole-source matching so multiline triple-quoted wrappers are
        handled correctly.

        Example:
            >>> r = SQLRewriter()
            >>> r.strip('from sqlty import SQL\\nq = SQL[tuple[int]]("--sql\\\\nSELECT 1::INTEGER")')
            'q = "--sql\\\\nSELECT 1::INTEGER"'
        """
        sql_wrapper_removed = False
        replacements: list[tuple[int, int, str]] = []

        for match in _SQL_WRAPPER_PATTERN.finditer(source):
            inner_string = match.group("inner")
            content = _extract_string_content(inner_string)
            # Only strip auto-generated wrappers (--sql + whitespace)
            if content is not None and _is_sqlty_managed(content):
                replacements.append(
                    (match.start("wrapper"), match.end("wrapper"), inner_string)
                )
                sql_wrapper_removed = True

        result = source
        for start, end, replacement in reversed(replacements):
            result = result[:start] + replacement + result[end:]

        if sql_wrapper_removed:
            result = self._remove_unused_imports(result)

        return result

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

# Match string literals that might contain --sql.
# Handles: """...""", '''...''', "...", '...'
# Triple-quoted alternatives come first so they match before single-quoted.
# Negative lookbehind ensures we skip prefixed strings (f"...", r"...", b"...", etc.)
_SQL_STRING_PATTERN = re.compile(
    r'(?<![a-zA-Z])(?P<str>'
    r'""".*?"""'        # triple-double-quoted
    r"|'''.*?'''"       # triple-single-quoted
    r'|"[^"]*"'         # double-quoted
    r"|'[^']*'"         # single-quoted
    r')',
    re.DOTALL,
)

# Match f-strings and t-strings (PEP 750, Python 3.14+) that contain --sql
# (for emitting warnings). Both use interpolation so cannot be statically rewritten.
_FSTRING_SQL_PATTERN = re.compile(
    r"""[fFtT][rRbBuU]*(?:"{3}|'{3}|"|')--sql\b"""
    r"""|[rRbBuU]*[fFtT](?:"{3}|'{3}|"|')--sql\b"""
)

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



def _has_import(source: str, import_line: str) -> bool:
    """Check if source already contains a specific import."""
    return any(line.strip() == import_line for line in source.splitlines())


def _line_at(source: str, pos: int) -> int:
    """Return the 1-indexed line number for a character position in *source*."""
    return source[:pos].count("\n") + 1
