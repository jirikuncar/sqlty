"""Tests for the ``--update-mdtest`` rewriting in tests/mdtest_runner.py."""

from __future__ import annotations

from pathlib import Path

import pytest

from tests.mdtest_runner import parse_mdtest_file

SOURCE = '''\
# Updater

## Grows to several lines

```python
q = """--sql
SELECT id::INTEGER FROM users"""
# rewritten-file:
```

## Shrinks to one line

```python
q = "--sql\\nSELECT name::TEXT FROM users"
# rewritten: stale line one
# rewritten: stale line two
# rewritten: stale line three
```

## Keeps indentation

```python
def f():
    q = "--sql\\nSELECT 1::INTEGER"
    return q
# rewritten:
# stripped: stale
```

## Unchanged

```python
q = "--sql\\nSELECT 1::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")
```
'''

EXPECTED = '''\
# Updater

## Grows to several lines

```python
q = """--sql
SELECT id::INTEGER FROM users"""
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("""--sql
# rewritten-file: SELECT id::INTEGER FROM users""")
```

## Shrinks to one line

```python
q = "--sql\\nSELECT name::TEXT FROM users"
# rewritten: q = SQL[tuple[str]]("--sql\\nSELECT name::TEXT FROM users")
```

## Keeps indentation

```python
def f():
    q = "--sql\\nSELECT 1::INTEGER"
    return q
# rewritten: def f():
# rewritten:     q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")
# rewritten:     return q
# stripped: def f():
# stripped:     q = "--sql\\nSELECT 1::INTEGER"
# stripped:     return q
```

## Unchanged

```python
q = "--sql\\nSELECT 1::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")
```
'''


def _update(path: Path, *, reverse: bool = False) -> None:
    cases = parse_mdtest_file(path, update=True)
    for case in reversed(cases) if reverse else cases:
        case.run()


@pytest.mark.parametrize("reverse", [False, True], ids=["in-order", "reversed"])
def test_update_rewrites_multiline_values(tmp_path: Path, reverse: bool) -> None:
    path = tmp_path / "sample.md"
    path.write_text(SOURCE)
    _update(path, reverse=reverse)
    assert path.read_text() == EXPECTED


def test_update_is_idempotent(tmp_path: Path) -> None:
    path = tmp_path / "sample.md"
    path.write_text(SOURCE)
    _update(path)
    _update(path)
    assert path.read_text() == EXPECTED
    for case in parse_mdtest_file(path):
        case.run()


def test_non_adjacent_assertions_stay_separate(tmp_path: Path) -> None:
    path = tmp_path / "sample.md"
    path.write_text(
        "## Two queries\n\n```python\n"
        'a = "--sql\\nSELECT 1::INTEGER"\n'
        "# inferred: tuple[int]\n"
        'b = "--sql\\nSELECT 2::INTEGER"\n'
        "# inferred: tuple[int]\n"
        "```\n"
    )
    (case,) = parse_mdtest_file(path)
    assert [(a.line, a.span) for a in case.assertions] == [(5, 1), (7, 1)]


def test_assertion_checks_its_own_python_block(tmp_path: Path) -> None:
    # The assertion lives in the first block; a later block in the same
    # section must not be used as its source.
    path = tmp_path / "sample.md"
    path.write_text(
        "## Before and after\n\n```python\n"
        'q = "--sql\\nSELECT 1::INTEGER"\n'
        '# rewritten: q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")\n'
        "```\n\n```python\n"
        'other = "--sql\\nSELECT name::TEXT FROM users"\n'
        "```\n"
    )
    (case,) = parse_mdtest_file(path)
    assert [a.block for a in case.assertions] == [0]
    case.run()


def test_decoded_drops_coding_cookie(tmp_path: Path) -> None:
    path = tmp_path / "sample.md"
    path.write_text(
        "## Codec\n\n```python\n"
        "# coding: sqlty\n\n"
        'q = "--sql\\nSELECT 1::INTEGER"\n'
        '# decoded: q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")\n'
        "```\n"
    )
    (case,) = parse_mdtest_file(path)
    case.run()


def test_check_lists_every_diagnostic_with_schema(tmp_path: Path) -> None:
    # `# check:` runs check_source on the block with the section's schema and
    # lists warnings as well as errors, one per line.
    path = tmp_path / "sample.md"
    path.write_text(
        "## Check\n\n`schema.sql`:\n\n```sql\n"
        "CREATE TABLE users (id INTEGER, name TEXT);\n"
        '```\n\n```toml\n[tool.sqlty]\nschema = "schema.sql"\n```\n\n'
        "```python\n"
        'a = SQL[tuple[int, str]]("--sql\\nSELECT * FROM users")\n'
        'b = "--sql\\nSELECT id FROM users"\n'
        "# check:\n"
        "```\n"
    )
    (case,) = parse_mdtest_file(path, update=True)
    case.run()
    assert path.read_text().endswith(
        '# check: [select-star] "SELECT * used — expand to explicit column list"\n'
        '# check: [stale-rewrite] "Source has --sql strings that aren\'t wrapped in SQL[T]"\n'
        "```\n"
    )
    (case,) = parse_mdtest_file(path)
    case.run()


def test_empty_check_means_no_diagnostics(tmp_path: Path) -> None:
    path = tmp_path / "sample.md"
    path.write_text(
        "## Clean\n\n```python\n"
        'q = SQL[tuple[int]]("--sql\\nSELECT id::INTEGER FROM users")\n'
        "# check:\n"
        "```\n"
    )
    (case,) = parse_mdtest_file(path)
    case.run()
