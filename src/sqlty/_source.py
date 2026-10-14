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
    parsed.imported_names   # absolute imports, local name to dotted name: {'D': 'decimal.Decimal'}
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


def is_any(node: ast.expr, aliases: dict[str, str]) -> bool:
    """Whether ``node`` is ``typing.Any``, quoted or in ``Annotated`` too.

    ``aliases`` is ``ParsedSource.typing_aliases``, so a bare ``Any`` that was
    not imported from ``typing`` is not one.

    Example:
        >>> aliases = ParsedSource("import typing as t\\nfrom typing import Any as A").typing_aliases
        >>> [is_any(ast.parse(s, mode="eval").body, aliases) for s in ("A", "t.Any", "'A'", "t.Annotated[A, 1]", "Any")]
        [True, True, True, True, False]
    """
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        try:
            node = ast.parse(node.value, mode="eval").body
        except SyntaxError:
            return False
    if (
        isinstance(node, ast.Subscript)
        and _typing_name(node.value, aliases) == "Annotated"
        and isinstance(node.slice, ast.Tuple)
    ):
        return is_any(node.slice.elts[0], aliases)
    return _typing_name(node, aliases) == "Any"


def _typing_name(node: ast.expr, aliases: dict[str, str]) -> str | None:
    """The ``typing`` member ``node`` names, if any."""
    if isinstance(node, ast.Name):
        return aliases.get(node.id)
    if (
        isinstance(node, ast.Attribute)
        and isinstance(node.value, ast.Name)
        and aliases.get(node.value.id) in _TYPING_MODULES
    ):
        return node.attr
    return None


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
        self.stacks: dict[int, _Stack] = {}
        self.types: dict[tuple[int | None, str], str | None] = {}
        self._stack: _Stack = ()
        self._depth = 0

    def visit(self, node: ast.AST) -> None:
        outer = self._stack
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            if not isinstance(node, ast.ClassDef):
                self._parameters(node)
            self._stack = (*outer, node)
        elif isinstance(node, (ast.Assign, ast.AnnAssign)):
            self._typed(node)
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            self.imports.append((self._depth, node))
        elif isinstance(node, ast.ClassDef):
            self.classes.append((self._depth, node))
        elif isinstance(node, ast.Call):
            self.calls.append((self._depth, node))
            self.stacks[id(node)] = self._stack
        elif self.bindings:
            self._bind(node)
        self._depth += 1
        self.generic_visit(node)
        self._depth -= 1
        self._stack = outer

    def _note(self, key: tuple[int | None, str], type_name: str | None) -> None:
        """Remember the class a name or ``self`` attribute holds; ``None`` once it is ambiguous."""
        known = self.types.get(key, type_name)
        self.types[key] = type_name if known == type_name else None

    def _parameters(self, node: ast.FunctionDef | ast.AsyncFunctionDef) -> None:
        args = node.args
        for arg in [*args.posonlyargs, *args.args, *args.kwonlyargs]:
            self._note((id(node), arg.arg), _class_name(arg.annotation))

    def _typed(self, node: ast.Assign | ast.AnnAssign) -> None:
        """Record ``name = Cls(...)`` and ``name: Cls`` in the current scope.

        Also ``self.attr`` the same way inside a method, keyed by the class.
        """
        if isinstance(node, ast.Assign):
            targets = node.targets
            type_name = _class_name(node.value.func) if isinstance(node.value, ast.Call) else None
        else:
            targets = [node.target]
            type_name = _class_name(node.annotation)
            if type_name is None and isinstance(node.value, ast.Call):
                type_name = _class_name(node.value.func)
        scope = self._stack[-1] if self._stack else None
        for target in targets:
            if isinstance(target, ast.Name):
                self._note((id(scope) if scope is not None else None, target.id), type_name)
            elif (
                isinstance(target, ast.Attribute)
                and isinstance(target.value, ast.Name)
                and len(self._stack) > 1
                and isinstance(self._stack[-2], ast.ClassDef)
                and isinstance(scope, (ast.FunctionDef, ast.AsyncFunctionDef))
                and scope.args.args
                and scope.args.args[0].arg == target.value.id
            ):
                self._note((id(self._stack[-2]), f".{target.attr}"), type_name)

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


_Stack = tuple[ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef, ...]


def _class_name(node: ast.expr | None) -> str | None:
    """The dotted name an annotation or a called expression spells, if it is a plain one.

    Example:
        >>> _class_name(ast.parse("m.Db", mode="eval").body)
        'm.Db'
        >>> _class_name(ast.Constant("Db"))
        'Db'
    """
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        text = node.value.strip()
        return text if re.fullmatch(r"[A-Za-z_]\w*(\.[A-Za-z_]\w*)*", text) else None
    parts: list[str] = []
    while isinstance(node, ast.Attribute):
        parts.append(node.attr)
        node = node.value
    if isinstance(node, ast.Name):
        return ".".join([node.id, *reversed(parts)])
    return None


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
        return PsycopgScan(self.imports, scan.scopes, scan.stores, scan.values, self)

    @functools.cached_property
    def constants(self) -> dict[str, ast.expr | None]:
        """Module-level names to the value of their one assignment.

        A name bound more than once, or by an import, ``def``, ``class`` or augmented
        assignment, maps to ``None``. Only top-level statements are read.

        Example:
            >>> sorted(ParsedSource("A = 'x'\\nB = 1\\nB = 2\\ndef C(): ...").constants)
            ['A', 'B', 'C']
            >>> ParsedSource("A = 'x'\\nB = 1\\nB = 2").constants['B'] is None
            True
        """
        found: dict[str, ast.expr | None] = {}
        for node in self.tree.body if self.tree is not None else []:
            bound: dict[str, ast.expr | None] = {}
            if isinstance(node, ast.Assign) and all(isinstance(t, ast.Name) for t in node.targets):
                bound = {t.id: node.value for t in node.targets if isinstance(t, ast.Name)}
            elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
                bound = {node.target.id: node.value}
            elif isinstance(node, ast.AugAssign) and isinstance(node.target, ast.Name):
                bound = {node.target.id: None}
            elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                bound = {node.name: None}
            elif isinstance(node, (ast.Import, ast.ImportFrom)):
                bound = {(a.asname or a.name).partition(".")[0]: None for a in node.names}
            for name, value in bound.items():
                found[name] = None if name in found else value
        return found

    @functools.cached_property
    def functions(self) -> list[ast.FunctionDef | ast.AsyncFunctionDef]:
        """Module-level function definitions."""
        body = self.tree.body if self.tree is not None else []
        return [n for n in body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]

    @functools.cached_property
    def methods(self) -> list[tuple[ast.ClassDef, ast.FunctionDef | ast.AsyncFunctionDef]]:
        """Functions defined directly in a class body, with their class."""
        return [
            (cls, node)
            for cls in self.classes
            for node in cls.body
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        ]

    def scopes_of(self, call: ast.Call) -> _Stack:
        """The functions and classes enclosing ``call``, outermost first."""
        return self._scan.stacks.get(id(call), ())

    def type_of(self, stack: _Stack, name: str, attribute: bool = False) -> str | None:
        """The class a variable holds, from ``name = Cls(...)`` or ``name: Cls`` in scope.

        ``stack`` is ``scopes_of(call)``. With ``attribute``, ``name`` is an attribute of
        ``self`` and the class of the innermost enclosing method is searched instead.
        """
        types = self._scan.types
        if attribute:
            cls = next((s for s in reversed(stack) if isinstance(s, ast.ClassDef)), None)
            return types.get((id(cls), f".{name}")) if cls is not None else None
        for scope in [*(s for s in reversed(stack) if not isinstance(s, ast.ClassDef)), None]:
            key = (id(scope) if scope is not None else None, name)
            if key in types:
                return types[key]
        return None

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
    def imported_names(self) -> dict[str, str]:
        """Local names bound by absolute imports, to the dotted names they stand for; the outermost binding wins.

        Example:
            >>> ParsedSource("import a.b, c as d\\nfrom e.f import g as h").imported_names
            {'a': 'a', 'd': 'c', 'h': 'e.f.g'}
        """
        names: dict[str, str] = {}
        for node in self.imports:
            if isinstance(node, ast.ImportFrom):
                if node.module and not node.level:
                    for a in node.names:
                        names.setdefault(a.asname or a.name, f"{node.module}.{a.name}")
            else:
                for a in node.names:
                    root = a.name.partition(".")[0]
                    names.setdefault(a.asname or root, a.name if a.asname else root)
        return names

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

    def any_annotations(self, import_style: str | None = None) -> list[str]:
        """Source of each ``T`` in ``SQL[T](...)`` that is ``typing.Any`` (see ``is_any``).

        Example:
            >>> ParsedSource("import typing as t\\nq = SQL[t.Any](x)\\nr = SQL[int](y)").any_annotations()
            ['t.Any']
        """
        wrapper = _wrapper_name(import_style)
        if wrapper is None or "Any" not in self.text or not self.may_wrap(import_style):
            return []
        aliases = self.typing_aliases
        annotations = set()
        for call in self.calls:
            func = call.func
            if not isinstance(func, ast.Subscript) or not is_any(func.slice, aliases):
                continue
            name = ast.unparse(func.value)
            if (name == wrapper or name.endswith(f".{wrapper}")) and (
                segment := ast.get_source_segment(self.text, func.slice)
            ):
                annotations.add(segment)
        return sorted(annotations)

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
