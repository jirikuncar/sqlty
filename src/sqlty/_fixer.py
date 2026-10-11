"""``sqlty check --fix``: apply the fixable rules to Python source.

Built from the ``sqlty.core`` functions, so it behaves the same on both
backends. User-defined row types (``SQL[User]``) are never changed.
"""

from __future__ import annotations

import re
from collections.abc import Callable

from sqlty._lint import RuleConfig
from sqlty._rewriter import ImportStyle, _has_import, _header_end, _is_sqlty_managed, _query

_QUOTES = ('"""', "'''", '"', "'")
# ``"--sql SELECT ..."`` opening a plain string literal; raw, f- and t-strings are left alone.
_INLINE_SQL_COMMENT = re.compile(r"""(?<!\w)([uU]?(?:\"\"\"|'''|"|'))--sql (?=\S)""")


def fix_source(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | None = None,
    import_style: str | None = None,
    untyped: bool = False,
    rules: dict[str, str] | None = None,
) -> str:
    """Fix ``inline-sql-comment``, ``stale-rewrite``, ``select-star``, ``stale-type`` and
    ``untyped-query``.

    Example:
        >>> fix_source('from sqlty import SQL\\nq = SQL[tuple[str]]("--sql\\\\nSELECT 1::INTEGER")')
        'from sqlty import SQL\\nq = SQL[tuple[int]]("--sql\\\\nSELECT 1::INTEGER")'
    """
    import sqlty.core as core

    enabled = RuleConfig(overrides=rules).is_enabled
    style = ImportStyle.parse(import_style) if import_style else ImportStyle.default()
    if enabled("inline-sql-comment"):
        source = _INLINE_SQL_COMMENT.sub(r"\1--sql\\n", source)
    if enabled("stale-rewrite"):
        source = core.rewrite_source(
            source,
            dialect=dialect,
            schema_sql=schema_sql,
            import_style=import_style,
            untyped=untyped,
        )

    def infer(content: str) -> str | None:
        try:
            return core.infer_type(_query(content), dialect=dialect, schema_sql=schema_sql)
        except Exception:
            return None

    introduced: set[str] = set()

    def fix(match: re.Match[str]) -> str:
        annotated, literal = match["type"], match["inner"]
        quote = next(q for q in _QUOTES if literal.startswith(q))
        content = literal[len(quote) : -len(quote)]
        if not _is_sqlty_managed(content):
            return match[0]
        if schema_sql and enabled("select-star"):
            content = _expand_star(content, quote, core.expand_star, dialect, schema_sql)
        refresh = (annotated.startswith("tuple[") and enabled("stale-type")) or (
            annotated == style.untyped_name and enabled("untyped-query")
        )
        row_type = (infer(content) if refresh else None) or annotated
        if row_type != annotated:
            introduced.add(row_type)
        return f"{style.wrapper_name}[{row_type}]({match['space']}{quote}{content}{quote}{match['end']}"

    fixed = _wrapper_pattern(style).sub(fix, source)
    return _fix_imports(fixed, style, introduced) if fixed != source else source


def _wrapper_pattern(style: ImportStyle) -> re.Pattern[str]:
    return re.compile(
        rf"{re.escape(style.wrapper_name)}\[(?P<type>[^\n]*?)\]\((?P<space>\s*)"
        r"(?P<inner>"
        r'""".*?"""'
        r"|'''.*?'''"
        r'|"[^"]*"'
        r"|'[^']*'"
        r")(?P<end>\s*,?\s*\))",
        re.DOTALL,
    )


def _expand_star(
    content: str,
    quote: str,
    expand_star: Callable[..., str | None],
    dialect: str,
    schema_sql: str,
) -> str:
    """Replace ``*`` in the query part of a string literal, when that is safe to write back."""
    match = re.match(r"(\s*--sql(?:\\n|\n))(.*)", content, re.DOTALL)
    if match is None:
        return content
    prefix, query = match.groups()
    if "*" not in query or "\\" in query or quote[0] in query:
        return content
    try:
        expanded = expand_star(query, dialect=dialect, schema_sql=schema_sql)
    except Exception:
        return content
    multiline = len(quote) == 1 and "\n" in (expanded or "")
    if expanded is None or "\\" in expanded or quote[0] in expanded or multiline:
        return content
    return prefix + expanded


def _fix_imports(source: str, style: ImportStyle, introduced: set[str]) -> str:
    needed = [m for m in ("datetime", "uuid") if any(f"{m}." in t for t in introduced)]
    missing = [f"import {m}" for m in needed if not _has_import(source, f"import {m}")]
    untyped = style.untyped_import
    if untyped and _has_import(source, untyped):
        lines = source.splitlines(keepends=True)
        rest = "".join(line for line in lines if line.rstrip() != untyped)
        if not re.search(rf"\b{style.untyped_name}\b", rest):
            source = rest
    if missing:
        at = _header_end(source)
        source = source[:at] + "\n".join(missing) + "\n" + source[at:]
    return source
