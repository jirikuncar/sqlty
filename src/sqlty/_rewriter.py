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


@dataclass(frozen=True)
class ImportStyle:
    """Parsed import-style configuration.

    Determines the import line added to source and the wrapper prefix
    used around SQL type annotations.

    Supported styles::

        from sqlty import SQL          → SQL[T](...)
        from sqlty import SQL as Q     → Q[T](...)
        import sqlty                   → sqlty.SQL[T](...)
        import sqlty as sy             → sy.SQL[T](...)

    Configure via ``[tool.sqlty]`` in pyproject.toml::

        [tool.sqlty]
        import-style = "from sqlty import SQL as Q"
    """

    import_line: str
    """The import statement to inject (e.g. ``"from sqlty import SQL as Q"``)."""

    wrapper_name: str
    """The prefix for ``[T](...)``, e.g. ``"Q"`` or ``"sqlty.SQL"``."""

    @property
    def untyped_name(self) -> str:
        """How ``UNTYPED`` is spelled: ``"UNTYPED"`` or ``"sqlty.UNTYPED"``."""
        module, dot, _ = self.wrapper_name.rpartition(".")
        return f"{module}{dot}UNTYPED"

    @property
    def untyped_import(self) -> str | None:
        """Import line ``UNTYPED`` needs, or ``None`` when the module is imported."""
        return None if "." in self.wrapper_name else "from sqlty import UNTYPED"

    @classmethod
    def parse(cls, style: str) -> ImportStyle:
        """Parse an import-style string into an ImportStyle.

        Args:
            style: One of the four supported forms.

        Raises:
            ValueError: If the style string is not recognised.
        """
        style = style.strip()
        if style.startswith("from sqlty import SQL as "):
            alias = style[len("from sqlty import SQL as ") :].strip()
            return cls(import_line=style, wrapper_name=alias)
        if style == "from sqlty import SQL":
            return cls(import_line=style, wrapper_name="SQL")
        if style.startswith("import sqlty as "):
            alias = style[len("import sqlty as ") :].strip()
            return cls(import_line=style, wrapper_name=f"{alias}.SQL")
        if style == "import sqlty":
            return cls(import_line=style, wrapper_name="sqlty.SQL")
        msg = f"Unsupported import-style: {style!r}"
        raise ValueError(msg)

    @classmethod
    def default(cls) -> ImportStyle:
        """Return the default import style: ``from sqlty import SQL``."""
        return cls(import_line="from sqlty import SQL", wrapper_name="SQL")

    def make_wrapper_pattern(self) -> re.Pattern[str]:
        """Build a regex that matches ``{wrapper_name}[T]("...")``.

        The string may sit on its own lines, as formatters lay out long calls.
        """
        escaped = re.escape(self.wrapper_name)
        return re.compile(
            rf"(?P<wrapper>{escaped}\[.*?\]\(\s*"
            r"(?P<inner>"
            r'""".*?"""'
            r"|'''.*?'''"
            r'|"[^"]*"'
            r"|'[^']*'"
            r")\s*\))",
            re.DOTALL,
        )


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
        import_style: Import style string (e.g. ``"from sqlty import SQL as Q"``).
            Controls the import line injected and the wrapper prefix.
            See :class:`ImportStyle` for supported forms.
        untyped: Wrap row-returning queries whose row type can't be inferred as
            ``SQL[UNTYPED](...)`` instead of leaving them as plain strings.
    """

    def __init__(
        self,
        dialect: str = "postgres",
        schema_path: str | None = None,
        codec: TypeCodec | None = None,
        import_style: str | None = None,
        untyped: bool = False,
    ) -> None:
        from pathlib import Path

        self._analyzer = SQLAnalyzer(
            dialect=dialect,
            schema_path=Path(schema_path) if schema_path else None,
        )
        self._codec = codec
        self._import_style = (
            ImportStyle.parse(import_style) if import_style else ImportStyle.default()
        )
        self._wrapper_pattern = self._import_style.make_wrapper_pattern()
        self._untyped = untyped

    def _infer_type(self, sql_content: str) -> str:
        """Infer the Python return type from a SQL query string.

        Passes the content directly to the SQL parser, letting SQL comment
        semantics handle the ``--sql`` prefix naturally:

        - ``--sql\\nSELECT ...`` → ``\\n`` becomes real newline; parser sees
          ``--sql`` as comment and ``SELECT ...`` as the query.
        - ``--sql<newline>SELECT ...`` → triple-quoted; parser sees comment + query.
        - ``--sql SELECT ...``  → entire line is a SQL comment → empty query.
        """
        try:
            result = self._analyzer.generate_return_type(_query(sql_content), codec=self._codec)
            return result if result else "Any"
        except Exception:
            return "Any"

    def _is_untyped(self, sql_content: str) -> bool:
        """Whether ``SQL[UNTYPED]`` fits: rows are expected but their type is unknown.

        Requires the ``--sql`` prefix to end its line; the space form is a SQL comment.
        """
        return sql_content.removeprefix("--sql").startswith(
            ("\\n", "\n", "\r\n")
        ) and self._analyzer.returns_rows(_query(sql_content))

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
        wrapper_name = self._import_style.wrapper_name
        import_line = self._import_style.import_line

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
        has_sql_import = _has_import(source, import_line)
        has_datetime_import = _has_import(source, "import datetime")
        has_uuid_import = _has_import(source, "import uuid")
        untyped_import = self._import_style.untyped_import
        needs_untyped_import = False
        needs_sql_import = False
        needs_datetime = False
        needs_uuid = False

        # Pre-compute existing wrapper spans to skip
        wrapper_spans = [(m.start(), m.end()) for m in self._wrapper_pattern.finditer(source)]

        # Collect all replacements, then apply right-to-left
        replacements: list[tuple[int, int, str]] = []
        for match in _SQL_STRING_PATTERN.finditer(source):
            start, end = match.start(), match.end()

            # Skip matches inside existing wrappers
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
                if not (self._untyped and self._is_untyped(content.strip())):
                    continue
                inferred = self._import_style.untyped_name
                needs_untyped_import = True

            replacement = f"{wrapper_name}[{inferred}]({string_literal})"
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
            imports_to_add.append(import_line)
        if needs_untyped_import and untyped_import and not _has_import(source, untyped_import):
            imports_to_add.append(untyped_import)

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

        for match in self._wrapper_pattern.finditer(source):
            inner_string = match.group("inner")
            content = _extract_string_content(inner_string)
            # Only strip auto-generated wrappers (--sql + whitespace)
            if content is not None and _is_sqlty_managed(content):
                replacements.append((match.start("wrapper"), match.end("wrapper"), inner_string))
                sql_wrapper_removed = True

        result = source
        for start, end, replacement in reversed(replacements):
            result = result[:start] + replacement + result[end:]

        if sql_wrapper_removed:
            result = self._remove_unused_imports(result)

        return result

    def _remove_unused_imports(self, source: str) -> str:
        """Remove sqlty/datetime/uuid imports if no longer referenced in source."""
        lines = source.splitlines(keepends=True)
        result: list[str] = []
        import_line = self._import_style.import_line
        wrapper_name = self._import_style.wrapper_name
        untyped_import = self._import_style.untyped_import
        body = source  # full source for reference checking
        untyped_used = any("UNTYPED" in line and line.strip() != untyped_import for line in lines)

        for line in lines:
            stripped = line.strip()

            if stripped == untyped_import and not untyped_used:
                continue

            # Remove sqlty import if wrapper no longer appears
            if stripped == import_line and f"{wrapper_name}[" not in body:
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
    r"(?<![a-zA-Z])(?P<str>"
    r'""".*?"""'  # triple-double-quoted
    r"|'''.*?'''"  # triple-single-quoted
    r'|"[^"]*"'  # double-quoted
    r"|'[^']*'"  # single-quoted
    r")",
    re.DOTALL,
)

# Match f-strings and t-strings (PEP 750, Python 3.14+) that contain --sql
# (for emitting warnings). Both use interpolation so cannot be statically rewritten.
_FSTRING_SQL_PATTERN = re.compile(
    r"""[fFtT][rRbBuU]*(?:"{3}|'{3}|"|')--sql\b"""
    r"""|[rRbBuU]*[fFtT](?:"{3}|'{3}|"|')--sql\b"""
)


def _query(sql_content: str) -> str:
    """Turn a source-text ``--sql\\n`` escape into a real newline for the parser."""
    if sql_content.startswith("--sql\\n"):
        return sql_content.replace("\\n", "\n", 1)
    return sql_content


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
