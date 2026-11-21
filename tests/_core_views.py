"""What the tests read of ``sqlty._core``, the extension's public API: the rule table and the CLI."""

from __future__ import annotations

import importlib.metadata
import sys
from collections.abc import Callable
from typing import NamedTuple

from sqlty import _core
from sqlty.core import HelperIndex, ParsedSource


class Rule(NamedTuple):
    name: str
    default_severity: str  # "error", "warn" or "ignore"
    fixable: bool
    description: str
    group: str


RULES: dict[str, Rule] = {name: Rule(name, *rest) for name, *rest in _core.rules()}
NAME_RULES: tuple[str, ...] = tuple(_core.name_rules())


def _console_script() -> Callable[[list[str] | None], None]:
    """The ``sqlty`` command as the package declares it (``[project.scripts]``)."""
    [script] = importlib.metadata.entry_points(group="console_scripts", name="sqlty")
    return script.load()


def main(argv: list[str]) -> None:
    """Run ``sqlty`` with ``argv`` in this process; a non-zero exit code is a ``SystemExit``."""
    _console_script()(argv)


def helper_sites(parsed: ParsedSource, helpers: HelperIndex | None) -> list[tuple]:
    """The helper calls of a module as the engine reads them (``_core.module_calls``)."""
    return _core.module_calls(
        parsed.text,
        version=sys.version_info[:2],
        index=None if helpers is None else helpers.rust(),
        name=parsed.module,
        module=parsed.module,
    )
