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
from typing import TYPE_CHECKING, Literal, assert_never

if TYPE_CHECKING:
    from sqlty._type_codec import TypeCodec

AssertionKind = Literal[
    "inferred",
    "rewritten",
    "rewritten-file",
    "stripped",
    "expanded",
    "error",
    "decoded",
    "warning",
]

# Regex to match assertion comments in python blocks — alternatives must match AssertionKind.
ASSERTION_RE = re.compile(
    r"^#\s+(inferred|rewritten|rewritten-file|stripped|expanded|error|decoded|warning):\s*(.*)$"
)


class MDTestFailure(Exception):
    """Raised when an mdtest assertion fails."""

    def __init__(self, *, test: str, kind: AssertionKind, expected: str, actual: str) -> None:
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

    kind: AssertionKind
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

    def _get_sqlty_config(self) -> dict:
        """Get the sqlty config, supporting both [sqlty] and [tool.sqlty] formats.

        Dedicated mdtest files use ``[sqlty]``, but README.md uses
        ``[tool.sqlty]`` (the pyproject.toml format). Both are supported.
        """
        cfg = self.config.get("sqlty")
        if cfg:
            return cfg
        return self.config.get("tool", {}).get("sqlty", {})

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
            case "rewritten":
                return self._rewrite()
            case "rewritten-file":
                return self._rewrite_file()
            case "stripped":
                return self._strip()
            case "expanded":
                return self._expand_star()
            case "error":
                return self._check_error()
            case "decoded":
                return self._decode()
            case "warning":
                return self._rewrite_warning()
            case _ as unreachable:
                assert_never(unreachable)

    def _infer_type(self) -> str:
        """Compute inferred type from the SQL block(s) in this test case."""
        from sqlty._sql_analyzer import SQLAnalyzer

        sqlty_config = self._get_sqlty_config()
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

        # Build codec from config if specified
        codec: TypeCodec | None = None
        codec_config = sqlty_config.get("codec")
        if codec_config:
            codec = self._build_codec(codec_config)

        # Use the last SQL block as the query
        if not self.sql_blocks:
            return "None"

        sql = self.sql_blocks[-1]
        result = analyzer.generate_return_type(sql, codec=codec)

        # Clean up temp schema file
        if schema_path and schema_path.exists():
            schema_path.unlink()

        return str(result) if result is not None else "None"

    @staticmethod
    def _build_codec(codec_config: dict | str) -> TypeCodec | None:
        """Build a TypeCodec from TOML config.

        Supports:
            codec = "tuple"                    → TupleCodec (default)
            codec = {template = "list[{types}]"}  → TemplateCodec
        """
        from sqlty._type_codec import TupleCodec

        if isinstance(codec_config, str):
            if codec_config == "tuple":
                return TupleCodec()
            return None

        if isinstance(codec_config, dict):
            template = codec_config.get("template")
            if template:
                # Inline template codec for testing custom formats
                class _TemplateCodec:
                    def __init__(self, tmpl: str) -> None:
                        self._template = tmpl

                    def format_type(self, column_types: list[str]) -> str:
                        return self._template.replace("{types}", ", ".join(column_types))

                return _TemplateCodec(template)

        return None

    def _build_rewriter(self):
        """Build an SQLRewriter from test case config."""
        from sqlty._rewriter import SQLRewriter

        sqlty_config = self._get_sqlty_config()
        dialect = sqlty_config.get("dialect", "postgres")

        # Handle schema file from file blocks
        schema_path_str = None
        schema_file = sqlty_config.get("schema")
        if schema_file and schema_file in self.files:
            import tempfile

            tmp = Path(tempfile.mktemp(suffix=".sql"))
            tmp.write_text(self.files[schema_file])
            schema_path_str = str(tmp)

        codec = None
        codec_config = sqlty_config.get("codec")
        if codec_config:
            codec = self._build_codec(codec_config)

        # Handle import-style config
        import_style = sqlty_config.get("import-style")

        return SQLRewriter(
            dialect=dialect,
            schema_path=schema_path_str,
            codec=codec,
            import_style=import_style,
            untyped=sqlty_config.get("untyped") == "sentinel",
        )

    def _rewrite(self) -> str:
        """Compute rewritten form of source lines above the assertion.

        For `# rewritten:`, we rewrite just the non-assertion code lines
        in the python block and return the rewritten code (without imports).
        """
        if not self.python_blocks:
            return ""

        rewriter = self._build_rewriter()
        source = self._extract_source_lines()
        result = rewriter.rewrite(source)
        # Strip import lines — assertions check only the code transformation
        style = rewriter._import_style
        return _strip_import_lines(
            result.source, style.import_line, style.untyped_import or style.import_line
        ).strip()

    def _rewrite_file(self) -> str:
        """Compute rewritten form including imports (whole-file output).

        For `# rewritten-file:`, we rewrite the non-assertion code lines
        and return the complete result including injected imports.
        Consecutive `# rewritten-file:` lines are merged by the parser
        into a single multi-line expected value.
        """
        if not self.python_blocks:
            return ""

        rewriter = self._build_rewriter()
        source = self._extract_source_lines()
        result = rewriter.rewrite(source)
        return result.source.strip()

    def _rewrite_warning(self) -> str:
        """Return the first warning from the rewriter.

        For `# warning:`, we rewrite the source and return the first
        warning message (e.g. f-string skipped).
        """
        if not self.python_blocks:
            return ""

        rewriter = self._build_rewriter()
        source = self._extract_source_lines()
        result = rewriter.rewrite(source)
        if result.warnings:
            return result.warnings[0]
        return ""

    def _strip(self) -> str:
        """Compute stripped form of source lines above the assertion.

        For `# stripped:`, we strip SQL[T](...) wrappers from code lines
        and return the result (without removed imports).
        """
        if not self.python_blocks:
            return ""

        rewriter = self._build_rewriter()
        source = self._extract_source_lines()
        result = rewriter.strip(source)
        return result.strip()

    def _extract_source_lines(self) -> str:
        """Extract non-assertion source lines from the last python block."""
        source = self.python_blocks[-1]
        source_lines = []
        for line in source.splitlines():
            stripped = line.strip()
            if ASSERTION_RE.match(stripped):
                continue
            source_lines.append(line)
        return "\n".join(source_lines)

    def _expand_star(self) -> str:
        """Compute expanded star form of the SQL query using schema.

        For `# expanded:`, we expand SELECT * into explicit column list
        using the loaded schema.
        """
        from sqlty._sql_analyzer import SQLAnalyzer

        sqlty_config = self._get_sqlty_config()
        dialect = sqlty_config.get("dialect", "postgres")

        # Build schema if available
        schema_path = None
        schema_file = sqlty_config.get("schema")
        if schema_file and schema_file in self.files:
            import tempfile

            tmp = Path(tempfile.mktemp(suffix=".sql"))
            tmp.write_text(self.files[schema_file])
            schema_path = tmp

        analyzer = SQLAnalyzer(dialect=dialect, schema_path=schema_path)

        # Get the SQL query
        sql = self.sql_blocks[-1] if self.sql_blocks else ""
        result = analyzer.expand_star(sql)

        # Clean up
        if schema_path and schema_path.exists():
            schema_path.unlink()

        return result if result is not None else "None"

    def _decode(self) -> str:
        """Run the magic codec decoder on python source and return result.

        For `# decoded:`, we pass the non-assertion source lines through
        the decode() function and return the transformed code (without imports).
        """
        from sqlty._decode import decode

        source = self._extract_source_lines()
        result, _ = decode(source.encode("utf-8"))
        # Strip import lines for clean comparison
        return _strip_import_lines(result).strip()

    def _get_import_line(self) -> str:
        """Get the import line from the test's sqlty config."""
        from sqlty._rewriter import ImportStyle

        sqlty_config = self._get_sqlty_config()
        style_str = sqlty_config.get("import-style")
        if style_str:
            return ImportStyle.parse(style_str).import_line
        return "from sqlty import SQL"

    def _check_error(self) -> str:
        """Run checker and return the first diagnostic as a ty-style string.

        Expected format: [rule-name] "message"
        If no diagnostics are emitted, returns empty string.

        Without a ``sql`` block, the Python block itself is checked with
        ``check_source`` (so user types defined there are validated) and
        every diagnostic is returned, one per line.
        """
        if not self.sql_blocks:
            return self._check_source_errors()

        from sqlty._checker import Checker
        from sqlty._lint import RuleConfig
        from sqlty._sql_analyzer import SQLAnalyzer

        sqlty_config = self._get_sqlty_config()
        dialect = sqlty_config.get("dialect", "postgres")

        # Build schema if available
        schema_path = None
        schema_file = sqlty_config.get("schema")
        if schema_file and schema_file in self.files:
            import tempfile

            tmp = Path(tempfile.mktemp(suffix=".sql"))
            tmp.write_text(self.files[schema_file])
            schema_path = tmp

        analyzer = SQLAnalyzer(dialect=dialect, schema_path=schema_path)
        rule_config = RuleConfig.from_config(sqlty_config)
        checker = Checker(analyzer=analyzer, rule_config=rule_config)

        # Get the SQL query
        sql = self.sql_blocks[-1] if self.sql_blocks else ""
        has_schema = schema_path is not None or bool(analyzer.schema)

        # Parse annotation fields from config if provided
        annotation_fields = None
        type_name = "tuple"
        annotation_config = sqlty_config.get("annotation")
        if annotation_config and isinstance(annotation_config, dict):
            type_name = annotation_config.get("type", "tuple")
            fields_raw = annotation_config.get("fields", [])
            annotation_fields = [(f[0], f[1]) for f in fields_raw]

        # Run checks
        diagnostics = checker.check_query(
            sql,
            annotation_fields=annotation_fields,
            type_name=type_name,
            has_schema=has_schema,
        )

        # Clean up
        if schema_path and schema_path.exists():
            schema_path.unlink()

        if diagnostics:
            d = diagnostics[0]
            return f'[{d.rule}] "{d.message}"'
        return ""

    def _check_source_errors(self) -> str:
        """Run ``check_source`` on the Python block; one diagnostic per line."""
        from sqlty.core import check_source

        sqlty_config = self._get_sqlty_config()
        diagnostics = check_source(
            self._extract_source_lines(),
            dialect=sqlty_config.get("dialect", "postgres"),
            rules=sqlty_config.get("rules"),
            import_style=sqlty_config.get("import-style"),
        )
        return "\n".join(f'[{d["rule"]}] "{d["message"]}"' for d in diagnostics)


def _strip_import_lines(
    source: str, import_line: str = "from sqlty import SQL", *extra_import_lines: str
) -> str:
    """Remove import lines from rewriter output.

    The `# rewritten:` assertion checks only the code transformation,
    not import injection. Import lines are:
    - import datetime
    - import uuid
    - The configured import line (default: ``from sqlty import SQL``)
    """
    skip = {"import datetime", "import uuid", import_line, *extra_import_lines}
    lines = []
    for line in source.splitlines():
        stripped = line.strip()
        if stripped in skip:
            continue
        lines.append(line)
    return "\n".join(lines)


def _rewrite_line_in_md(md_path: Path, line_num: int, kind: AssertionKind, actual: str) -> None:
    """Rewrite assertion comment(s) in a .md file with the actual value.

    Handles multi-line assertions: scans forward from *line_num* to find
    all consecutive ``# {kind}:`` lines, then replaces them with the
    (possibly different number of) lines from *actual*.
    """
    lines = md_path.read_text().splitlines(keepends=True)
    idx = line_num - 1
    if not (0 <= idx < len(lines)):
        return

    # Preserve leading whitespace from the first assertion line
    stripped = lines[idx].lstrip()
    indent = lines[idx][: len(lines[idx]) - len(stripped)]

    # Find the extent of consecutive same-kind assertion lines
    prefix = f"# {kind}:"
    end_idx = idx + 1
    while end_idx < len(lines) and lines[end_idx].strip().startswith(prefix):
        end_idx += 1

    # Build replacement lines from actual value
    new_lines = [f"{indent}# {kind}: {part}\n" for part in actual.split("\n")]

    lines[idx:end_idx] = new_lines
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
                    raw_assertions: list[Assertion] = []
                    for py_line_idx, py_line in enumerate(code_block_lines):
                        match = ASSERTION_RE.match(py_line.strip())
                        if match:
                            kind = match.group(1)
                            expected = match.group(2).strip()
                            # The assertion's line in the .md file
                            # code_block started at some line, py_line_idx into it
                            assertion_md_line = line_num - len(code_block_lines) + py_line_idx
                            raw_assertions.append(
                                Assertion(
                                    kind=kind,
                                    expected=expected,
                                    line=assertion_md_line,
                                )
                            )
                    # Merge consecutive same-kind assertions into multi-line expected values.
                    # This allows whole-file assertions like:
                    #   # rewritten-file: from sqlty import SQL
                    #   # rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
                    merged = _merge_consecutive_assertions(raw_assertions)
                    assertions.extend(merged)
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


def collect_mdtest_file(md_path: Path, *, update: bool = False) -> list[MDTestItem]:
    """Collect test items from a markdown file for pytest."""
    test_cases = parse_mdtest_file(md_path, update=update)
    return [MDTestItem(tc.name, tc) for tc in test_cases]


def _merge_consecutive_assertions(assertions: list[Assertion]) -> list[Assertion]:
    """Merge consecutive same-kind assertions into multi-line expected values.

    When multiple ``# rewritten-file:`` (or any same-kind) lines appear in
    sequence, they represent a single multi-line expected value.  The lines
    are joined with ``\\n`` so the comparison works against multi-line output.
    """
    if not assertions:
        return []

    merged: list[Assertion] = []
    for assertion in assertions:
        if merged and merged[-1].kind == assertion.kind:
            prev = merged[-1]
            merged[-1] = Assertion(
                kind=prev.kind,
                expected=prev.expected + "\n" + assertion.expected,
                line=prev.line,  # keep first line number for update mode
            )
        else:
            merged.append(assertion)
    return merged


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
