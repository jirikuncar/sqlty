"""One Python source file, parsed and walked at most once.

Every check that needs the syntax tree of a file (helper indexing, helper
call sites, user-defined row types, ``# sqlty: ignore`` comments) reads it from
a ``ParsedSource`` instead of calling ``ast.parse`` / ``tokenize`` itself, so a
file costs one parse and one walk however many checks look at it.

Usage:
    parsed = ParsedSource(text, module="pkg.mod")
    parsed.tree             # ast.Module, parsed on first access; None on a syntax error
    parsed.imports          # every Import / ImportFrom node
    parsed.classes          # every ClassDef node
    parsed.calls            # every Call node
    parsed.typing_aliases   # local names of typing members: {'t': 'typing', 'A': 'Any'}
    parsed.ignores          # {line: _Ignore} for "# sqlty: ignore" comments

Pass the object wherever a function takes ``source: str | ParsedSource``
(``check_source``, ``HelperIndex.add``, ``find_helper_calls``,
``extract_type_definitions``); a plain ``str`` is wrapped with
``ParsedSource.of``. Build one per file and reuse it; never call
``ast.parse(parsed.text)``. Nothing is parsed until a property that needs the
tree is read: guard with the cheap ``text`` checks (``may_define_helpers``,
``has_ignore_comments``) first. ``imports``, ``classes`` and ``calls`` come
from the same single visitor pass, in ``ast.walk`` order (breadth first).
Module-level statements are ``tree.body``; ``functions`` lists the module-level
function definitions.

Example:
    >>> parsed = ParsedSource("import typing as t\\nclass A: ...\\nf(A)")
    >>> parsed.typing_aliases, [c.name for c in parsed.classes], len(parsed.calls)
    ({'t': 'typing'}, ['A'], 1)
    >>> ParsedSource("x = 1").may_define_helpers
    False
"""

from __future__ import annotations

import ast
import functools
import io
import re
import tokenize
from dataclasses import dataclass

from sqlty._psycopg import PsycopgScan

_TYPING_MODULES = frozenset({"typing", "typing_extensions"})
_GAP = r"(?:[\s\"'rRuU]|#[^\n]*)*"
_MARKER_RE = re.compile(rf"-{_GAP}-{_GAP}s{_GAP}q{_GAP}l", re.IGNORECASE)
_IGNORE_RE = re.compile(r"#\s*sqlty:\s*ignore\b(?:\[([^\]]*)\])?")


@dataclass(frozen=True)
class _Ignore:
    """A ``# sqlty: ignore`` comment; ``rules`` is ``None`` for a bare one."""

    line: int
    rules: tuple[str, ...] | None


def _wrapper_name(import_style: str | None) -> str | None:
    """How the engine spells the wrapper for ``import_style``; ``None`` when it rejects it."""
    style = (import_style or "").strip()
    if style in ("", "from sqlty import SQL"):
        return "SQL"
    if style == "import sqlty":
        return "sqlty.SQL"
    for prefix, suffix in (("from sqlty import SQL as ", ""), ("import sqlty as ", ".SQL")):
        if style.startswith(prefix):
            return style[len(prefix) :].strip() + suffix
    return None


class _Scan(ast.NodeVisitor):
    """Collects the nodes consumers need in one traversal, in ``ast.walk`` order."""

    def __init__(self, bindings: bool = False) -> None:
        self.imports: list[tuple[int, ast.Import | ast.ImportFrom]] = []
        self.classes: list[tuple[int, ast.ClassDef]] = []
        self.calls: list[tuple[int, ast.Call]] = []
        self.bindings = bindings
        self.scopes: list[ast.FunctionDef | ast.AsyncFunctionDef | ast.Lambda | ast.ClassDef] = []
        self.stores: dict[str, list[ast.Name]] = {}
        self.values: dict[int, ast.expr] = {}
        self._depth = 0

    def visit(self, node: ast.AST) -> None:
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            self.imports.append((self._depth, node))
        elif isinstance(node, ast.ClassDef):
            self.classes.append((self._depth, node))
        elif isinstance(node, ast.Call):
            self.calls.append((self._depth, node))
        elif self.bindings:
            self._bind(node)
        self._depth += 1
        self.generic_visit(node)
        self._depth -= 1

    def _bind(self, node: ast.AST) -> None:
        """Record what binds a name or opens a scope, for the psycopg composition scan."""
        if isinstance(node, ast.Name):
            if isinstance(node.ctx, ast.Store):
                self.stores.setdefault(node.id, []).append(node)
        elif isinstance(node, ast.Assign):
            if len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
                self.values[id(node.targets[0])] = node.value
        elif isinstance(node, ast.AnnAssign):
            if isinstance(node.target, ast.Name) and node.value is not None:
                self.values[id(node.target)] = node.value
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda, ast.ClassDef)):
            self.scopes.append(node)


def _breadth_first[T](found: list[tuple[int, T]]) -> list[T]:
    """Depth-first pre-order nodes in breadth-first order (stable per depth)."""
    return [node for _, node in sorted(found, key=lambda item: item[0])]


class ParsedSource:
    """Source text with its lazily parsed, cached syntax tree and facts.

    ``module`` is the dotted name of the file, used to resolve relative imports.
    """

    def __init__(self, text: str, module: str | None = None) -> None:
        self.text = text
        self.module = module

    @classmethod
    def of(cls, source: str | ParsedSource, module: str | None = None) -> ParsedSource:
        """``source`` itself when it is already parsed, else a new ``ParsedSource``."""
        return source if isinstance(source, ParsedSource) else cls(source, module)

    @functools.cached_property
    def tree(self) -> ast.Module | None:
        """The module, or ``None`` when the text is not valid Python."""
        try:
            return ast.parse(self.text)
        except SyntaxError, ValueError:
            return None

    @functools.cached_property
    def _scan(self) -> _Scan:
        scan = _Scan(bindings="psycopg" in self.text)
        if self.tree is not None:
            scan.visit(self.tree)
        return scan

    @functools.cached_property
    def imports(self) -> list[ast.Import | ast.ImportFrom]:
        """Every import statement, at any depth."""
        return _breadth_first(self._scan.imports)

    @functools.cached_property
    def classes(self) -> list[ast.ClassDef]:
        """Every class definition, at any depth."""
        return _breadth_first(self._scan.classes)

    @functools.cached_property
    def calls(self) -> list[ast.Call]:
        """Every call expression."""
        return _breadth_first(self._scan.calls)

    @functools.cached_property
    def psycopg(self) -> PsycopgScan:
        """What ``psycopg.sql`` compositions need: imports, scopes and single assignments.

        Collected in the same pass as ``imports``, and only when the text mentions ``psycopg``.
        """
        scan = self._scan
        return PsycopgScan(self.imports, scan.scopes, scan.stores, scan.values)

    @functools.cached_property
    def functions(self) -> list[ast.FunctionDef | ast.AsyncFunctionDef]:
        """Module-level function definitions."""
        body = self.tree.body if self.tree is not None else []
        return [n for n in body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]

    @functools.cached_property
    def typing_aliases(self) -> dict[str, str]:
        """Local names for ``typing`` / ``typing_extensions`` and their members, to originals."""
        aliases: dict[str, str] = {}
        for node in self.imports:
            if isinstance(node, ast.ImportFrom):
                if node.module in _TYPING_MODULES:
                    aliases |= {a.asname or a.name: a.name for a in node.names}
            else:
                aliases |= {
                    a.asname or a.name: a.name for a in node.names if a.name in _TYPING_MODULES
                }
        return aliases

    @functools.cached_property
    def marker_names(self) -> tuple[set[str], set[str]]:
        """Local spellings of ``Annotated`` and of the ``sqlty.SQL`` marker.

        Example:
            >>> ParsedSource("import typing as t\\nfrom sqlty import SQL as Q").marker_names
            ({'t.Annotated'}, {'Q'})
        """
        annotated: set[str] = set()
        marker: set[str] = set()
        for node in self.imports:
            if isinstance(node, ast.ImportFrom):
                if node.level:
                    continue
                for alias in node.names:
                    local = alias.asname or alias.name
                    if node.module in _TYPING_MODULES and alias.name == "Annotated":
                        annotated.add(local)
                    elif node.module == "sqlty" and alias.name == "SQL":
                        marker.add(local)
            else:
                for alias in node.names:
                    local = alias.asname or alias.name
                    if alias.name in _TYPING_MODULES:
                        annotated.add(f"{local}.Annotated")
                    elif alias.name == "sqlty":
                        marker.add(f"{local}.SQL")
        return annotated, marker

    @functools.cached_property
    def may_define_helpers(self) -> bool:
        """Whether the text can define a marked helper (needs ``Annotated`` and ``sqlty``)."""
        return "Annotated" in self.text and "sqlty" in self.text and "SQL" in self.text

    def may_wrap(self, import_style: str | None = None) -> bool:
        """Whether the text can hold a ``SQL[T](...)`` call, spelled as ``import_style`` says."""
        prefix = "from sqlty import SQL as "
        style = (import_style or "").strip()
        return (style[len(prefix) :].strip() if style.startswith(prefix) else "SQL") in self.text

    def may_need_rust_check(self, import_style: str | None = None) -> bool:
        """Whether the Rust checker can report anything for the text.

        Every Rust rule starts from a ``--sql`` / ``-- sql`` marker at the start of a
        string (``mysql-sql-marker``, ``stale-rewrite``, ``inline-sql-comment``,
        ``unparsed-query``, ``fstring-skipped``, ``tstring-skipped``,
        ``unchecked-query``, ``select-star*``, ``stale-type``, ``untyped-query``); the
        wrapper call (``SQL[``, spelled as ``import_style`` says) and ``UNTYPED`` count
        too, which also covers a marker split over concatenated literals in a call. A marker
        split over implicit concatenation anywhere else (``"--" "sql\\n..."``) matches
        with quotes, prefixes, whitespace and comments allowed between its characters.
        An ``import_style`` the engine rejects always needs the call, so that it raises.

        Example:
            >>> ParsedSource("x = 'SELECT 1'").may_need_rust_check()
            False
            >>> ParsedSource("x = '--sql\\nSELECT 1'").may_need_rust_check()
            True
            >>> ParsedSource("q = Q[UNTYPED]").may_need_rust_check("from sqlty import SQL as Q")
            True
        """
        wrapper = _wrapper_name(import_style)
        text = self.text
        return (
            wrapper is None
            or f"{wrapper}[" in text
            or "UNTYPED" in text
            or _MARKER_RE.search(text) is not None
        )

    @property
    def has_ignore_comments(self) -> bool:
        """Whether the text can hold a ``# sqlty: ignore`` comment."""
        return "sqlty:" in self.text

    @functools.cached_property
    def ignores(self) -> dict[int, _Ignore]:
        """``# sqlty: ignore`` comments by line, read from COMMENT tokens only.

        Tokenizes only when the text contains ``sqlty:``.
        """
        found: dict[int, _Ignore] = {}
        if not self.has_ignore_comments:
            return found
        tokens = tokenize.generate_tokens(io.StringIO(self.text).readline)
        try:
            for tok in tokens:
                if tok.type != tokenize.COMMENT or (match := _IGNORE_RE.match(tok.string)) is None:
                    continue
                names = match.group(1)
                rules = (
                    None
                    if names is None
                    else tuple(n.strip() for n in names.split(",") if n.strip())
                )
                found[tok.start[0]] = _Ignore(tok.start[0], rules)
        except tokenize.TokenError, SyntaxError:
            pass
        return found
