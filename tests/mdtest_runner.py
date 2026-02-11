"""Markdown test runner — snapshot-based assertion engine for .md test files.

Parses markdown files into test cases and runs assertions against sqlty's
SQLAnalyzer. Follows the same compare-or-update pattern as inline-snapshot:
normal mode fails on drift, --update-mdtest rewrites .md files with actual values.

Inspired by ty's mdtest framework:
https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md
"""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import pytest

# Regex to match assertion comments in python blocks
ASSERTION_RE = re.compile(r"^#\s+(inferred|rewritten|stripped|expanded|error):\s*(.*)$")


class MDTestFailure(Exception):
    """Raised when an mdtest assertion fails."""

    def __init__(self, *, test: str, kind: str, expected: str, actual: str) -> None:
        self.test = test
        self.kind = kind
        self.expected = expected
        self.actual = actual

    def __str__(self) -> str:
        return (
            f"Assertion `# {self.kind}:` failed\n"
            f"  Expected: {self.expected}\n"
            f"  Actual:   {self.actual}"
        )


@dataclass
class Assertion:
    """A single assertion comment from a .md file."""

    kind: str  # "inferred", "rewritten", "stripped", "expanded", "error"
    expected: str  # The expected value after the prefix
    line: int  # Line number in the .md file (1-indexed, for update mode)


@dataclass
class MDTestCase:
    """A single test case extracted from a markdown heading."""

    name: str
    md_path: Path
    config: dict = field(default_factory=dict)
    sql_blocks: list[str] = field(default_factory=list)
    python_blocks: list[str] = field(default_factory=list)
    files: dict[str, str] = field(default_factory=dict)
    assertions: list[Assertion] = field(default_factory=list)
    update: bool = False

    def run(self) -> None:
        """Execute all assertions in this test case."""
        for assertion in self.assertions:
            actual = self._compute(assertion)
            if actual == assertion.expected:
                continue
            if self.update:
                _rewrite_line_in_md(self.md_path, assertion.line, assertion.kind, actual)
            else:
                raise MDTestFailure(
                    test=self.name,
                    kind=assertion.kind,
                    expected=assertion.expected,
                    actual=actual,
                )

    def _compute(self, assertion: Assertion) -> str:
        match assertion.kind:
            case "inferred":
                return self._infer_type()
            case _:
                raise ValueError(f"Unknown assertion kind: {assertion.kind}")

    def _infer_type(self) -> str:
        """Compute inferred type from the SQL block(s) in this test case."""
        from sqlty._sql_analyzer import SQLAnalyzer

        sqlty_config = self.config.get("sqlty", {})
        dialect = sqlty_config.get("dialect", "postgres")

        # Check for schema file in config
        schema_path = None
        schema_file = sqlty_config.get("schema")
        if schema_file and schema_file in self.files:
            # Write schema to a temp file for SQLAnalyzer
            import tempfile

            tmp = Path(tempfile.mktemp(suffix=".sql"))
            tmp.write_text(self.files[schema_file])
            schema_path = tmp

        analyzer = SQLAnalyzer(dialect=dialect, schema_path=schema_path)

        # Use the last SQL block as the query
        if not self.sql_blocks:
            return "None"

        sql = self.sql_blocks[-1]
        result = analyzer.generate_return_type(sql)

        # Clean up temp schema file
        if schema_path and schema_path.exists():
            schema_path.unlink()

        return str(result) if result is not None else "None"


def _rewrite_line_in_md(md_path: Path, line_num: int, kind: str, actual: str) -> None:
    """Rewrite an assertion comment in a .md file with the actual value."""
    lines = md_path.read_text().splitlines(keepends=True)
    # line_num is 1-indexed
    idx = line_num - 1
    if 0 <= idx < len(lines):
        # Preserve leading whitespace
        stripped = lines[idx].lstrip()
        indent = lines[idx][: len(lines[idx]) - len(stripped)]
        lines[idx] = f"{indent}# {kind}: {actual}\n"
        md_path.write_text("".join(lines))


def parse_mdtest_file(md_path: Path, *, update: bool = False) -> list[MDTestCase]:
    """Parse a markdown file into a list of test cases.

    Each heading (##, ###, etc.) that contains code blocks becomes a test case.
    TOML blocks set configuration. SQL blocks provide the query. Python blocks
    contain assertion comments.

    Configuration inherits from parent sections to children (like ty's mdtest).
    """
    content = md_path.read_text()
    lines = content.splitlines()

    test_cases: list[MDTestCase] = []

    # State machine for parsing
    current_heading: str | None = None
    current_level: int = 0
    current_config: dict = {}
    config_stack: list[tuple[int, dict]] = []  # (level, config) for inheritance

    # Accumulate blocks for current section
    sql_blocks: list[str] = []
    python_blocks: list[str] = []
    assertions: list[Assertion] = []
    files: dict[str, str] = {}
    pending_file_label: str | None = None

    in_code_block = False
    code_block_lang = ""
    code_block_lines: list[str] = []

    for line_idx, line in enumerate(lines):
        line_num = line_idx + 1  # 1-indexed

        # Check for code fence
        if line.strip().startswith("```"):
            if in_code_block:
                # End of code block
                block_content = "\n".join(code_block_lines)

                if code_block_lang == "toml":
                    try:
                        parsed = tomllib.loads(block_content)
                        current_config = _merge_config(current_config, parsed)
                    except Exception:
                        pass
                elif code_block_lang == "sql":
                    if pending_file_label:
                        files[pending_file_label] = block_content
                        pending_file_label = None
                    else:
                        sql_blocks.append(block_content)
                elif code_block_lang in ("python", "py"):
                    # Extract assertions from python block
                    for py_line_idx, py_line in enumerate(code_block_lines):
                        match = ASSERTION_RE.match(py_line.strip())
                        if match:
                            kind = match.group(1)
                            expected = match.group(2).strip()
                            # The assertion's line in the .md file
                            # code_block started at some line, py_line_idx into it
                            assertion_md_line = (
                                line_num - len(code_block_lines) + py_line_idx
                            )
                            assertions.append(
                                Assertion(
                                    kind=kind,
                                    expected=expected,
                                    line=assertion_md_line,
                                )
                            )
                    if pending_file_label:
                        files[pending_file_label] = block_content
                        pending_file_label = None
                    else:
                        python_blocks.append(block_content)

                in_code_block = False
                code_block_lang = ""
                code_block_lines = []
            else:
                # Start of code block
                in_code_block = True
                lang = line.strip().removeprefix("```").strip()
                code_block_lang = lang
                code_block_lines = []
            continue

        if in_code_block:
            code_block_lines.append(line)
            continue

        # Check for file label: `filename.ext`:
        file_label_match = re.match(r"^`([^`]+)`:\s*$", line.strip())
        if file_label_match:
            pending_file_label = file_label_match.group(1)
            continue

        # Check for heading
        heading_match = re.match(r"^(#{1,6})\s+(.+)$", line)
        if heading_match:
            # Save previous test case if it has assertions
            if current_heading and assertions:
                test_cases.append(
                    MDTestCase(
                        name=current_heading,
                        md_path=md_path,
                        config=dict(current_config),
                        sql_blocks=list(sql_blocks),
                        python_blocks=list(python_blocks),
                        files=dict(files),
                        assertions=list(assertions),
                        update=update,
                    )
                )

            level = len(heading_match.group(1))
            heading_text = heading_match.group(2).strip()

            # Pop config stack back to parent level
            while config_stack and config_stack[-1][0] >= level:
                config_stack.pop()

            # Push current config as parent
            if current_heading:
                config_stack.append((current_level, dict(current_config)))

            # Inherit config from parent
            if config_stack:
                current_config = dict(config_stack[-1][1])

            current_heading = heading_text
            current_level = level

            # Reset block accumulators for new section
            sql_blocks = []
            python_blocks = []
            assertions = []
            files = {}

    # Don't forget the last test case
    if current_heading and assertions:
        test_cases.append(
            MDTestCase(
                name=current_heading,
                md_path=md_path,
                config=dict(current_config),
                sql_blocks=list(sql_blocks),
                python_blocks=list(python_blocks),
                files=dict(files),
                assertions=list(assertions),
                update=update,
            )
        )

    return test_cases


def collect_mdtest_file(
    md_path: Path, *, update: bool = False
) -> list[MDTestItem]:
    """Collect test items from a markdown file for pytest."""
    test_cases = parse_mdtest_file(md_path, update=update)
    return [
        MDTestItem(tc.name, tc)
        for tc in test_cases
    ]


def _merge_config(base: dict, override: dict) -> dict:
    """Deep merge override into base config."""
    result = dict(base)
    for key, value in override.items():
        if key in result and isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = _merge_config(result[key], value)
        else:
            result[key] = value
    return result


class MDTestItem:
    """Wrapper for pytest collection — holds name and test case."""

    def __init__(self, name: str, test_case: MDTestCase) -> None:
        self.name = name
        self.test_case = test_case
