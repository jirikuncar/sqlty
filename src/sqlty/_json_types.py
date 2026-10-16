"""TypedDicts for JSON columns: what ``json-types = "typeddict"`` writes into a module.

``rewrite`` types a JSON column ``typing.Any``. Where the engine knows the shape of the
JSON a query builds (``jsonb_build_object()``, ``jsonb_agg()``, ``to_jsonb()``), the
``typing.Any`` items of a ``SQL[tuple[...]]`` annotation become nested types instead, with a
``TypedDict`` per object. Each class is written after the imports, below a
``# sqlty: generated`` line, and ``strip`` removes them again.
"""

from __future__ import annotations

import ast
import json
import keyword
import re
import typing

from sqlty._core import analyze_columns
from sqlty._source import _wrapper_name

if typing.TYPE_CHECKING:
    from sqlty._core import Schema

MARKER = "# sqlty: generated"
_BLOCK = (
    rf"{MARKER}\r?\n(?:class [^\r\n]*:\r?\n(?:    [^\r\n]*\r?\n)+"
    r"|[A-Za-z_]\w* = typing\.TypedDict\([^\r\n]*\)\r?\n)"
)
_GROUP = re.compile(rf"(\r?\n){{2}}{_BLOCK}(?:(?:\r?\n){{2}}{_BLOCK})*(?:\r?\n){{2}}")
_SCALARS = {
    "integer": "int",
    "number": "float",
    "string": "str",
    "boolean": "bool",
    "null": "None",
    "any": "typing.Any",
    "never": "typing.Any",
}


def add_json_types(
    source: str,
    *,
    dialect: str = "postgres",
    schema_sql: str | Schema | None = None,
    import_style: str | None = None,
) -> str:
    """``source`` with the ``typing.Any`` row items of JSON columns of known shape typed.

    Example:
        >>> source = (
        ...     "import typing\\nfrom sqlty import SQL\\n"
        ...     "q = SQL[tuple[int, typing.Any]]"
        ...     "(\\"--sql\\\\nSELECT 1::INTEGER AS id, jsonb_build_object('n', 1) AS doc\\")\\n"
        ... )
        >>> print(add_json_types(source))
        import typing
        from sqlty import SQL
        <BLANKLINE>
        <BLANKLINE>
        # sqlty: generated
        class Doc(typing.TypedDict):
            n: int
        <BLANKLINE>
        <BLANKLINE>
        q = SQL[tuple[int, Doc]]("--sql\\nSELECT 1::INTEGER AS id, jsonb_build_object('n', 1) AS doc")
        <BLANKLINE>
    """
    wrapper = _wrapper_name(import_style)
    try:
        tree = ast.parse(source)
    except SyntaxError, ValueError:
        return source
    if wrapper is None:
        return source
    module = _Module(tree, source)
    edits: list[tuple[int, int, str]] = []
    for node in ast.walk(tree):
        if not (
            isinstance(node, ast.Call)
            and isinstance(node.func, ast.Subscript)
            and ast.unparse(node.func.value) == wrapper
            and node.args
            and isinstance(node.args[0], ast.Constant)
            and isinstance(node.args[0].value, str)
        ):
            continue
        row = node.func.slice
        if not (isinstance(row, ast.Subscript) and ast.unparse(row.value) == "tuple"):
            continue
        items = row.slice.elts if isinstance(row.slice, ast.Tuple) else [row.slice]
        if not any(ast.unparse(item) in ("typing.Any", "Any") for item in items):
            continue
        try:
            columns = analyze_columns(node.args[0].value, dialect=dialect, schema_sql=schema_sql)
        except RuntimeError:
            continue
        if columns is None or len(columns) != len(items):
            continue
        for item, (name, _, _, nullable, shape) in zip(items, columns, strict=True):
            if shape is None or ast.unparse(item) not in ("typing.Any", "Any"):
                continue
            value = {"shape": json.loads(shape), "nullable": nullable is True}
            if value["shape"] == "any":
                continue
            edits.append((*module.span(item), module.type_of(value, _pascal(name))))
    if not edits:
        return source
    for start, end, text in sorted(edits, reverse=True):
        source = source[:start] + text + source[end:]
    return module.with_classes(source)


def strip_json_types(source: str) -> str:
    """``source`` without the classes ``add_json_types`` wrote.

    Example:
        >>> strip_json_types("import typing\\n\\n\\n# sqlty: generated\\nclass A(typing.TypedDict):\\n    a: int\\n\\n\\nq = 1\\n")
        'import typing\\nq = 1\\n'
    """
    return _GROUP.sub("", source) if MARKER in source else source


class _Module:
    """The top-level names of a module and the classes generated for it so far."""

    def __init__(self, tree: ast.Module, source: str) -> None:
        self.tree = tree
        self.lines = source.splitlines(keepends=True)
        self.taken: set[str] = set()
        self.existing: dict[str, str] = {}
        self.new: dict[str, str] = {}
        generated = {
            index + 1 for index, line in enumerate(self.lines) if line.rstrip("\r\n") == MARKER
        }
        for stmt in tree.body:
            names = [
                target.id for target in getattr(stmt, "targets", []) if isinstance(target, ast.Name)
            ]
            if isinstance(stmt, ast.ClassDef | ast.FunctionDef | ast.AsyncFunctionDef):
                names.append(stmt.name)
            if isinstance(stmt, ast.Import | ast.ImportFrom):
                names += [(alias.asname or alias.name).split(".")[0] for alias in stmt.names]
            self.taken.update(names)
            if stmt.lineno - 1 in generated and names:
                self.existing[names[0]] = ast.unparse(stmt)

    def span(self, node: ast.expr) -> tuple[int, int]:
        """Character offsets of ``node`` in the source (``ast`` counts UTF-8 bytes)."""
        assert node.end_lineno is not None and node.end_col_offset is not None

        def offset(line: int, column: int) -> int:
            before = sum(len(text) for text in self.lines[: line - 1])
            return before + len(self.lines[line - 1].encode()[:column].decode())

        return offset(node.lineno, node.col_offset), offset(node.end_lineno, node.end_col_offset)

    def type_of(self, value: dict[str, typing.Any], name: str) -> str:
        """The Python type of a JSON value of the engine's shape; objects get a class ``name``."""
        shape = value["shape"]
        if isinstance(shape, str):
            text = _SCALARS.get(shape, "typing.Any")
        else:
            kind, body = next(iter(shape.items()))
            if kind == "array":
                text = f"list[{self.type_of(body, name + 'Item')}]"
            elif kind == "map":
                text = f"dict[str, {self.type_of(body, name + 'Value')}]"
            else:
                text = self.typed_dict(body, name)
        return f"{text} | None" if value["nullable"] and text != "None" else text

    def typed_dict(self, fields: list[dict[str, typing.Any]], name: str) -> str:
        """The name of a ``TypedDict`` class with ``fields``, written once per module."""
        types = []
        for field in fields:
            text = self.type_of(field["value"], name + _pascal(field["name"]))
            types.append(
                (field["name"], f"typing.NotRequired[{text}]" if field["optional"] else text)
            )
        for candidate in [name, *(f"{name}{n}" for n in range(2, 100))]:
            code = _class(candidate, types)
            body = ast.unparse(ast.parse(code).body[0])
            if self.existing.get(candidate) == body or self.new.get(candidate) == code:
                return candidate
            if candidate not in self.taken and candidate not in self.new:
                self.new[candidate] = code
                return candidate
        return "typing.Any"

    def with_classes(self, source: str) -> str:
        """``source`` with the new classes after the imports, or after the generated ones."""
        if not self.new:
            return source
        eol = "\r\n" if "\r\n" in source else "\n"
        blocks = "\n\n".join(f"{MARKER}\n{code}" for code in self.new.values())
        blocks = blocks.replace("\n", eol)
        groups = list(_GROUP.finditer(source))
        if groups:
            at = groups[-1].end()
            return source[:at] + blocks + eol * 2 + source[at:]
        header = [stmt for stmt in self.tree.body if isinstance(stmt, ast.Import | ast.ImportFrom)]
        line = header[-1].end_lineno if header else 0
        at = sum(len(text) for text in self.lines[: line or 0])
        if at and not source[:at].endswith("\n"):
            source = source[:at] + eol + source[at:]
            at += len(eol)
        return source[:at] + eol * 2 + blocks + eol * 2 + source[at:]


def _class(name: str, fields: list[tuple[str, str]]) -> str:
    if fields and all(key.isidentifier() and not keyword.iskeyword(key) for key, _ in fields):
        body = "".join(f"    {key}: {text}\n" for key, text in fields)
        return f"class {name}(typing.TypedDict):\n{body}"
    items = ", ".join(f"{key!r}: {text}" for key, text in fields)
    return f"{name} = typing.TypedDict({name!r}, {{{items}}})\n"


def _pascal(name: str) -> str:
    """``order_lines`` as ``OrderLines``; a name with no letters is ``Json``."""
    words = re.findall(r"[A-Za-z0-9]+", name)
    text = "".join(word[:1].upper() + word[1:] for word in words)
    if not text or text[0].isdigit():
        text = "Json" + text
    return text
