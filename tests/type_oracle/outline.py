"""The module tree type extraction reads: ``ast.parse`` with function bodies cut to the classes they hold."""

from __future__ import annotations

import ast
import warnings
import weakref

from sqlty._source import ParsedSource

Outline = tuple[ast.Module, list[ast.ClassDef]]

_outlines: weakref.WeakKeyDictionary[ParsedSource, Outline | None] = weakref.WeakKeyDictionary()


def outline(parsed: ParsedSource) -> Outline | None:
    """The cut module and its classes in ``ast.walk`` order, read once per ``parsed``.

    ``None`` when the text is not valid Python or Rust declines it (``parsed.facts``), as
    Rust then reads no types either.

    Example:
        >>> tree, classes = outline(ParsedSource("def f():\\n    x = 1\\n    class A: ...\\nclass B: ..."))
        >>> ast.unparse(tree).splitlines(), [c.name for c in classes]
        (['def f():', '', '    class A:', '        ...', '', 'class B:', '    ...'], ['B', 'A'])
    """
    if parsed not in _outlines:
        _outlines[parsed] = _read(parsed)
    return _outlines[parsed]


def classes(parsed: ParsedSource) -> list[ast.ClassDef]:
    """The classes of ``outline(parsed)``, none when there is no outline."""
    return [] if (found := outline(parsed)) is None else found[1]


def _read(parsed: ParsedSource) -> Outline | None:
    if parsed.facts is None:
        return None
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", SyntaxWarning)
        try:
            tree = ast.parse(parsed.text)
        except SyntaxError:
            return None
    for node in ast.walk(tree):
        if isinstance(node, ast.FunctionDef | ast.AsyncFunctionDef):
            node.body = _cut(node.body)
    return tree, [node for node in ast.walk(tree) if isinstance(node, ast.ClassDef)]


def _holds_class(node: ast.AST) -> bool:
    return any(isinstance(n, ast.ClassDef) for n in ast.walk(node))


def _cut(body: list[ast.stmt]) -> list[ast.stmt]:
    kept = [stmt for stmt in body if _holds_class(stmt)]
    for stmt in kept:
        if isinstance(stmt, ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef):
            continue
        for name in ("body", "orelse", "finalbody"):
            if isinstance(inner := getattr(stmt, name, None), list):
                setattr(stmt, name, _cut(inner))
        for name in ("handlers", "cases"):
            if isinstance(inner := getattr(stmt, name, None), list):
                setattr(stmt, name, [part for part in inner if _holds_class(part)])
                for part in getattr(stmt, name):
                    part.body = _cut(part.body)
    return kept
