r"""A Pygments lexer for Python that colors SQL in the strings ``sqlty check`` reads.

Installing sqlty registers it as ``sqlty``: ``pygmentize -l sqlty queries.py``, or map
``python`` blocks to it in your docs. The SQL tokens are those of ``highlight_source``, the
ones ``sqlty server`` and the playground color; the rest is Pygments' Python lexer.

Example:
    >>> from pygments.lexers import get_lexer_by_name
    >>> lexer = get_lexer_by_name("sqlty")
    >>> for kind, text in lexer.get_tokens('q = "--sql\\nSELECT 1"'):
    ...     print(str(kind).removeprefix("Token."), repr(text))
    Name 'q'
    Text ' '
    Operator '='
    Text ' '
    Literal.String.Double '"'
    Comment.Single '--sql'
    Literal.String.Escape '\\n'
    Keyword 'SELECT'
    Literal.String.Double ' '
    Literal.Number '1'
    Literal.String.Double '"'
    Text.Whitespace '\n'
"""

from __future__ import annotations

from collections.abc import Iterator

from pygments.lexer import Lexer
from pygments.lexers.python import PythonLexer
from pygments.token import Comment, Keyword, Name, Number, Operator, String, _TokenType

from sqlty.core import highlight_source

SQL_TOKENS: dict[str, _TokenType] = {
    "keyword": Keyword,
    "string": String.Single,
    "number": Number,
    "comment": Comment.Single,
    "operator": Operator,
    "parameter": Name.Variable,
}


class SQLTyLexer(Lexer):
    """Python, with the SQL of query strings colored by sqlty."""

    name = "SQLTy"
    aliases = ["sqlty"]

    def get_tokens_unprocessed(self, text: str) -> Iterator[tuple[int, _TokenType, str]]:
        # `highlight_source` counts UTF-8 bytes; Pygments counts characters.
        char = [i for i, c in enumerate(text) for _ in c.encode()] + [len(text)]
        spans = [
            (char[h.start], char[h.end], SQL_TOKENS[h.kind])
            for h in highlight_source(text, dialect=self.options.get("dialect", "postgres"))
        ]
        found = 0
        for start, kind, value in PythonLexer(**self.options).get_tokens_unprocessed(text):
            at, end = start, start + len(value)
            while at < end:
                while found < len(spans) and spans[found][1] <= at:
                    found += 1
                sql_start, sql_end, sql_kind = (
                    spans[found] if found < len(spans) else (end, end, kind)
                )
                if sql_start <= at:
                    upto = min(sql_end, end)
                    yield at, sql_kind, text[at:upto]
                else:
                    upto = min(sql_start, end)
                    yield at, kind, text[at:upto]
                at = upto
