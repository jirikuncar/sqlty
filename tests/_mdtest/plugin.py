"""pytest plugin: collect Markdown files under ``mdtest_paths`` as tests.

Each section with assertions is an item, run once per available backend
when it touches ``sqlty.core``::

    uv run pytest docs/guide/row-types.md
    uv run pytest 'tests/mdtest/check/user_types.md::TypedDict - field type mismatch[rs]'
    uv run pytest tests/mdtest --update-mdtest
"""

from __future__ import annotations

import os
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
    parser.addoption(
        "--sqlty-lib",
        choices=("py", "rs", "both"),
        default=os.environ.get("SQLTY_LIB", "both"),
        help="Backend to test: py (Python), rs (Rust), both (default)",
    )


def pytest_configure(config: pytest.Config) -> None:
    config.stash[BACKENDS_KEY] = available_backends(config.getoption("--sqlty-lib"))


def available_backends(choice: str) -> list[str]:
    if choice != "both":
        return [choice]
    try:
        import sqlty._core  # noqa: F401
    except ImportError:
        return ["py"]
    return ["py", "rs"]


def pytest_collect_file(parent: pytest.Collector, file_path: Path) -> MDTestFile | None:
    if file_path.suffix != ".md":
        return None
    roots: list[Path] = parent.config.getini("mdtest_paths")
    if any(file_path == root or root in file_path.parents for root in roots):
        return MDTestFile.from_parent(parent, path=file_path)
    return None


BACKENDS_KEY = pytest.StashKey[list[str]]()


class MDTestFile(pytest.File):
    def collect(self) -> Iterator[MDTestItem]:
        try:
            sections = parse(self.path)
        except ParseError as e:
            raise self.CollectError(str(e)) from None
        update = self.config.getoption("--update-mdtest")
        updater = Updater(self.path) if update else None
        backends = self.config.stash[BACKENDS_KEY]
        for section in sections:
            if not section.uses_backend:
                yield MDTestItem.from_parent(
                    self, name=section.name, section=section, updater=updater
                )
                continue
            forced = section.directives.get("backend")
            for name in backends:
                if forced in (None, name):
                    yield MDTestItem.from_parent(
                        self,
                        name=f"{section.name}[{name}]",
                        section=section,
                        updater=updater,
                        backend=name,
                    )


class MDTestItem(pytest.Item):
    def __init__(
        self,
        *,
        section: Section,
        updater: Updater | None,
        backend: str | None = None,
        **kwargs: object,
    ) -> None:
        super().__init__(**kwargs)  # type: ignore[arg-type]
        self.section = section
        self.updater = updater
        self.backend = backend
        if "skip" in section.directives:
            self.add_marker(pytest.mark.skip(reason=section.directives["skip"] or "mdtest:skip"))
        if "xfail" in section.directives:
            self.add_marker(pytest.mark.xfail(reason=section.directives["xfail"], strict=True))

    def runtest(self) -> None:
        from .runner import Run, backend

        run = Run(self.section, self.updater)
        if self.backend is None:
            run.run()
        else:
            with backend(self.backend):
                run.run()

    def repr_failure(
        self, excinfo: pytest.ExceptionInfo[BaseException], style: TracebackStyle | None = None
    ) -> str | TerminalRepr:
        from .runner import MDTestFailure

        if isinstance(excinfo.value, MDTestFailure):
            return f"{excinfo.value}\n\nrerun: uv run pytest '{self.nodeid}'"
        return super().repr_failure(excinfo, style)

    def reportinfo(self) -> tuple[Path, int, str]:
        return self.path, self.section.line - 1, f"mdtest: {self.name}"
