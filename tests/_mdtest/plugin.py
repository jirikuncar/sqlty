"""pytest plugin: collect Markdown files under ``mdtest_paths`` as tests.

Each section with assertions is an item::

    uv run pytest docs/guide/row-types.md
    uv run pytest 'tests/mdtest/check/user_types.md::TypedDict - field type mismatch'
    uv run pytest tests/mdtest --update-mdtest
"""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from _pytest._code.code import TerminalRepr, TracebackStyle

from .parser import ParseError, Section, parse
from .updater import Updater

# sqlty is imported lazily (runner) so coverage, which starts after -p plugins
# load, sees its module-level lines.


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addini("mdtest_paths", "Markdown files and directories run as mdtests", type="paths")
    parser.addoption(
        "--update-mdtest",
        action="store_true",
        help="Rewrite assertion comments in .md files with actual values",
    )


def pytest_collect_file(parent: pytest.Collector, file_path: Path) -> MDTestFile | None:
    if file_path.suffix != ".md":
        return None
    roots: list[Path] = parent.config.getini("mdtest_paths")
    if any(file_path == root or root in file_path.parents for root in roots):
        return MDTestFile.from_parent(parent, path=file_path)
    return None


class MDTestFile(pytest.File):
    def collect(self) -> Iterator[MDTestItem]:
        try:
            sections = parse(self.path)
        except ParseError as e:
            raise self.CollectError(str(e)) from None
        update = self.config.getoption("--update-mdtest")
        updater = Updater(self.path) if update else None
        for section in sections:
            yield MDTestItem.from_parent(self, name=section.name, section=section, updater=updater)


class MDTestItem(pytest.Item):
    def __init__(
        self,
        *,
        section: Section,
        updater: Updater | None,
        **kwargs: object,
    ) -> None:
        super().__init__(**kwargs)  # type: ignore[arg-type]
        self.section = section
        self.updater = updater
        if "skip" in section.directives:
            self.add_marker(pytest.mark.skip(reason=section.directives["skip"] or "mdtest:skip"))
        if "xfail" in section.directives:
            self.add_marker(pytest.mark.xfail(reason=section.directives["xfail"], strict=True))

    def runtest(self) -> None:
        from .runner import Run

        Run(self.section, self.updater).run()

    def repr_failure(
        self, excinfo: pytest.ExceptionInfo[BaseException], style: TracebackStyle | None = None
    ) -> str | TerminalRepr:
        from .runner import MDTestFailure

        if isinstance(excinfo.value, MDTestFailure):
            return f"{excinfo.value}\n\nrerun: uv run pytest '{self.nodeid}'"
        return super().repr_failure(excinfo, style)

    def reportinfo(self) -> tuple[Path, int, str]:
        return self.path, self.section.line - 1, f"mdtest: {self.name}"
