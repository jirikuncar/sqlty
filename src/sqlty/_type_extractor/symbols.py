"""The names the annotations of a module go through, read from source across checked modules."""

from __future__ import annotations

import ast
import typing

from sqlty._source import ParsedSource
from sqlty._type_extractor.ast_fields import _ast_type, _module_types

if typing.TYPE_CHECKING:
    from sqlty._helpers import HelperIndex


class ModuleTypes:
    """The aliases and ``NewType``s of a module, then the types it imports from checked modules.

    Nothing is imported or evaluated: an imported name is resolved from the source of the
    module that defines it, in that module's own names.

    Example:
        >>> from sqlty._helpers import HelperIndex
        >>> from sqlty._type_extractor.ast_fields import _ast_type
        >>> index = HelperIndex()
        >>> _ = index.add("ids", "from typing import NewType\\nUserId = NewType('UserId', int)")
        >>> app = ParsedSource("import ids\\nfrom ids import UserId\\nMaybe = UserId | None")
        >>> names = ModuleTypes(app, index)
        >>> _ast_type(ast.Name("Maybe"), {}, names), _ast_type(ast.parse("ids.UserId").body[0].value, {}, names)
        (ids.UserId | None, ids.UserId)
    """

    def __init__(
        self, parsed: ParsedSource, helpers: HelperIndex | None, module: str | None = None
    ) -> None:
        self.parsed = parsed
        self.helpers = helpers
        self.module = module or "__main__"
        tree = parsed.tree
        self.local = {} if tree is None else _module_types(tree, parsed.typing_aliases)
        self._newtypes: dict[str, typing.Any] = {}
        self._imported: dict[str, typing.Any] = {}

    def get(self, name: str, /) -> ast.expr | typing.Any:
        if isinstance(found := self.local.get(name), ast.Assign):
            return self._newtype(name, typing.cast(ast.Call, found.value).args[1])
        if found is not None:
            return found
        if self.helpers is None:
            return None
        if name not in self._imported:
            self._imported[name] = self.helpers.type_of(self.parsed, name)
        return self._imported[name]

    def _newtype(self, name: str, supertype: ast.expr) -> typing.Any:
        """``NewType(name, supertype)``, built once, ``None`` when ``supertype`` is unknown."""
        if name not in self._newtypes:
            # A NewType of itself is unknown.
            self._newtypes[name] = None
            parsed = self.parsed
            base = _ast_type(supertype, parsed.typing_aliases, self, parsed.imported_names)
            if base is not None:
                new_type: typing.Any = typing.NewType
                self._newtypes[name] = new_type(name, base)
                self._newtypes[name].__module__ = self.module
        return self._newtypes[name]
