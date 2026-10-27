"""The mdtest format itself: parsing rules, error matching and ``--update-mdtest``."""

from __future__ import annotations

from pathlib import Path

import pytest

from tests._mdtest.parser import ParseError, parse
from tests._mdtest.runner import MDTestFailure, Run
from tests._mdtest.updater import Updater

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
```

## Keeps indentation

```python
def f():
    q = "--sql\\nSELECT 1::INTEGER"
    return q
# rewritten:
# stripped: stale
```

## Errors move above their line

```python
q = SQL[tuple[str]]("--sql\\nSELECT 1::INTEGER")  # error: [narrow-type]
# error: [select-star]
r = "--sql\\nSELECT 2::INTEGER"
```

## SQL assertions

```sql
SELECT id::INTEGER FROM users
-- inferred: tuple[str]
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

## Errors move above their line

```python
# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\\nSELECT 1::INTEGER")
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
r = "--sql\\nSELECT 2::INTEGER"
```

## SQL assertions

```sql
SELECT id::INTEGER FROM users
-- inferred: tuple[int]
```
'''


def _run(path: Path, *, update: bool = False, reverse: bool = False) -> None:
    sections = parse(path)
    updater = Updater(path) if update else None
    for section in reversed(sections) if reverse else sections:
        Run(section, updater).run()


def _write(tmp_path: Path, text: str) -> Path:
    path = tmp_path / "sample.md"
    path.write_text(text)
    return path


@pytest.mark.parametrize("reverse", [False, True], ids=["in-order", "reversed"])
def test_update(tmp_path: Path, reverse: bool) -> None:
    path = _write(tmp_path, SOURCE)
    _run(path, update=True, reverse=reverse)
    assert path.read_text() == EXPECTED
    _run(path)


def test_update_once_per_section(tmp_path: Path) -> None:
    path = _write(tmp_path, SOURCE)
    sections = parse(path)
    updater = Updater(path)
    for section in sections + sections:
        Run(section, updater).run()
    assert path.read_text() == EXPECTED


def test_failure_report(tmp_path: Path) -> None:
    path = _write(tmp_path, SOURCE)
    reports = []
    for section in parse(path):
        with pytest.raises(MDTestFailure) as excinfo:
            Run(section, None).run()
        reports.append(str(excinfo.value))
    report = "\n".join(reports)
    assert f"{path}:8: `# rewritten-file:` failed" in report
    assert f"{path}:34: unexpected diagnostic [stale-rewrite]" in report
    assert f"{path}:34: unmatched assertion `# error: [select-star]`" in report


def test_names_join_headings(tmp_path: Path) -> None:
    path = _write(tmp_path, "# T\n\n## A\n\n### B\n\n```sql\nSELECT 1\n-- inferred: x\n```\n")
    assert [s.name for s in parse(path)] == ["A - B"]


def test_literate_blocks_concatenate(tmp_path: Path) -> None:
    path = _write(
        tmp_path,
        '## A\n\n```py\nx = 1\n```\n\nThen:\n\n```py\nq = "--sql\\nSELECT 1::INTEGER"\n'
        "# rewritten:\n```\n",
    )
    (section,) = parse(path)
    (unit,) = section.units
    assert unit.code.text == 'x = 1\nq = "--sql\\nSELECT 1::INTEGER"'
    assert unit.code.md_lines == [4, 10]


def test_units_end_at_their_assertions(tmp_path: Path) -> None:
    path = _write(
        tmp_path,
        "## A\n\n```py\nsetup = 1\n```\n\n```py\na = 1\n# stripped:\n```\n\n"
        "```py\nafter = 1\n```\n\n```py\nb = 1\n# stripped:\n```\n\n"
        "```sql\nSELECT 1\n-- inferred:\n```\n\n```sql\nSELECT 2\n-- inferred:\n```\n",
    )
    (section,) = parse(path)
    assert [(u.lang, u.code.text) for u in section.units] == [
        ("python", "setup = 1\na = 1"),
        ("python", "after = 1\nb = 1"),
        ("sql", "SELECT 1"),
        ("sql", "SELECT 2"),
    ]


def test_config_and_files_are_inherited(tmp_path: Path) -> None:
    path = _write(
        tmp_path,
        '## Group\n\n```toml\n[tool.sqlty]\nschema = "s.sql"\n```\n\n`s.sql`:\n\n'
        "```sql\nCREATE TABLE t (id INT);\n```\n\n### Test\n\n"
        "```sql\nSELECT id FROM t\n-- inferred: tuple[int]\n```\n",
    )
    (section,) = parse(path)
    assert section.settings == {"schema": "s.sql"}
    assert section.files == {"s.sql": "CREATE TABLE t (id INT);"}


def test_ignored_and_prose_blocks(tmp_path: Path) -> None:
    path = _write(
        tmp_path,
        "## A\n\n```bash\nsqlty check .\n```\n\n```python ignore\nbroken(\n```\n\n"
        "    ```python\n    indented()\n    ```\n",
    )
    assert parse(path) == []


@pytest.mark.parametrize(
    ("text", "message"),
    [
        ("## A\n\n```toml\n[sqlty]\n```\n", "config must be a [tool.sqlty] table"),
        ("## A\n\n```toml\n[tool.sqlty]\ncodec = 1\n```\n", "unknown key 'codec' in [tool.sqlty]"),
        (
            "## A\n\n```toml\n[tool.sqlty.rules]\nnope = 'warn'\n```\n",
            "unknown rule 'nope' in [tool.sqlty.rules]",
        ),
        ("## A\n\n<!-- mdtest:nope -->\n", "unknown directive mdtest:nope"),
        ("## A\n\n```py\n# inferred: x\n```\n", "write `-- inferred:` inside the sql block"),
        ("## A\n\n```py\nx = 1\n# error: [r]\n```\n", "`# error:` has no code line below it"),
        ("## A\n\n```py\n", "unclosed code block"),
        (
            "## A\n\n```toml\n[tool.sqlty]\nschema = 's.sql'\n```\n\n```sql\nSELECT 1\n"
            "-- inferred: x\n```\n",
            "schema 's.sql' has no `s.sql`: block",
        ),
        (
            "## A\n\n```sql\nSELECT 1\n-- inferred: x\n```\n\n## A\n\n```sql\nSELECT 1\n"
            "-- inferred: x\n```\n",
            "duplicate test name 'A'",
        ),
    ],
)
def test_parse_errors(tmp_path: Path, text: str, message: str) -> None:
    with pytest.raises(ParseError, match=message.replace("[", r"\[").replace("(", r"\(")):
        parse(_write(tmp_path, text))
