"""Root conftest.py — pytest plugin for mdtest .md file discovery."""

from __future__ import annotations

from pathlib import Path

import pytest

from tests.mdtest_runner import MDTestFailure, parse_mdtest_file


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addoption(
        "--update-mdtest",
        action="store_true",
        default=False,
        help="Update assertion comments in .md files with actual computed values",
    )


def pytest_collect_file(
    parent: pytest.Collector, file_path: Path
) -> MDTestFile | None:
    if file_path.suffix == ".md" and "mdtest" in file_path.parts:
        return MDTestFile.from_parent(parent, path=file_path)
    return None


class MDTestFile(pytest.File):
    def collect(self):  # type: ignore[override]
        update = self.config.getoption("--update-mdtest", default=False)
        test_cases = parse_mdtest_file(self.path, update=update)
        for tc in test_cases:
            yield MDTestItem.from_parent(self, name=tc.name, test_case=tc)


class MDTestItem(pytest.Item):
    def __init__(self, name: str, parent: pytest.Collector, test_case: object) -> None:
        super().__init__(name, parent)
        self.test_case = test_case  # MDTestCase

    def runtest(self) -> None:
        self.test_case.run()  # type: ignore[attr-defined]

    def repr_failure(
        self,
        excinfo: pytest.ExceptionInfo[BaseException],
        style: str | None = None,
    ) -> str:
        if isinstance(excinfo.value, MDTestFailure):
            return str(excinfo.value)
        return super().repr_failure(excinfo)  # type: ignore[return-value]

    def reportinfo(self):  # type: ignore[override]
        return self.path, None, f"mdtest: {self.name}"
