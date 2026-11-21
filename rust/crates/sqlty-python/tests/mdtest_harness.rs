//! The mdtest runner itself: failure reports, `xfail`, and `MDTEST_UPDATE` rewriting values
//! and `error` comments in place.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_methods,
    reason = "a test writes and reads its own Markdown"
)]
#![allow(dead_code, reason = "the shared runner serves every crate's kinds")]

#[path = "../../../mdtest/harness.rs"]
mod harness;
#[path = "../../../mdtest/parser.rs"]
mod parser;
#[path = "../../../mdtest/python.rs"]
mod python;
#[path = "../../../mdtest/sql.rs"]
mod sql;

use std::path::{Path, PathBuf};

const SOURCE: &str = r#"# Updater

## Grows to several lines

```python
q = """--sql
SELECT id::INTEGER FROM users"""
# rewritten-file:
```

## Shrinks to one line

```python
q = "--sql\nSELECT name::TEXT FROM users"
# rewritten: stale line one
# rewritten: stale line two
```

## Keeps indentation

```python
def f():
    q = "--sql\nSELECT 1::INTEGER"
    return q
# rewritten:
# stripped: stale
```

## Errors move above their line

```python
q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER")  # error: [narrow-type]
# error: [select-star]
r = "--sql\nSELECT 2::INTEGER"
```

## Driver types

```toml
[tool.sqlty]
driver = "psycopg"
```

```sql
SELECT '[1,2)'::INT4RANGE
-- inferred: tuple[str]
```
"#;

const EXPECTED: &str = r#"# Updater

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
q = "--sql\nSELECT name::TEXT FROM users"
# rewritten: q = SQL[tuple[str]]("--sql\nSELECT name::TEXT FROM users")
```

## Keeps indentation

```python
def f():
    q = "--sql\nSELECT 1::INTEGER"
    return q
# rewritten: def f():
# rewritten:     q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten:     return q
# stripped: def f():
# stripped:     q = "--sql\nSELECT 1::INTEGER"
# stripped:     return q
```

## Errors move above their line

```python
# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER")
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
r = "--sql\nSELECT 2::INTEGER"
```

## Driver types

```toml
[tool.sqlty]
driver = "psycopg"
```

```sql
SELECT '[1,2)'::INT4RANGE
-- inferred: tuple[psycopg.types.range.Range[int]]
```
"#;

const RUNNER: python::Python = python::Python { sqruff: None };

fn write(name: &str, text: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.md");
    std::fs::write(&path, text).unwrap();
    path
}

fn run(path: &Path, update: bool) -> Vec<String> {
    harness::run_file(&RUNNER, path.parent().unwrap(), path, update)
}

#[test]
fn update_writes_the_actual_values() {
    let path = write("update", SOURCE);
    assert_eq!(run(&path, true), Vec::<String>::new());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), EXPECTED);
    assert_eq!(run(&path, false), Vec::<String>::new());
}

#[test]
fn failures_name_the_assertion_and_the_diagnostic() {
    let report = run(&write("report", SOURCE), false).join("\n");
    for expected in [
        "sample.md:8: `# rewritten-file:` failed",
        "sample.md:34: unexpected diagnostic [stale-rewrite]",
        "sample.md:34: unmatched assertion `# error: [select-star]`",
        "sample.md:46: `-- inferred:` failed",
    ] {
        assert!(report.contains(expected), "{expected} not in\n{report}");
    }
}

#[test]
fn a_passing_xfail_is_a_failure() {
    let source = "## A\n\n<!-- mdtest:xfail fixed -->\n\n```toml\n[tool.sqlty]\ndriver = \"psycopg\"\n```\n\n```sql\nSELECT 1::INTEGER\n-- inferred: tuple[int]\n```\n\n## B\n\n<!-- mdtest:skip -->\n\n```python\nq = 1\n# stripped: wrong\n```\n";
    let path = write("xfail", source);
    assert_eq!(
        run(&path, false),
        vec!["sample.md:1: `A` passes: drop its mdtest:xfail".to_string()]
    );
}

#[test]
fn update_keeps_the_escapes_of_the_code_it_does_not_assert() {
    let source = r#"# Escapes

```python
a = "--sql\nSELECT \061::INTEGER"
# rewritten: stale
b = "--sql\nSELECT '\d'::TEXT"
```
"#;
    let path = write("escapes", source);
    run(&path, true);
    let written = std::fs::read_to_string(&path).unwrap();
    for kept in [
        r#"a = "--sql\nSELECT \061::INTEGER""#,
        r#"b = "--sql\nSELECT '\d'::TEXT""#,
    ] {
        assert!(written.contains(kept), "{kept} not in\n{written}");
    }
}

#[test]
fn update_keeps_the_bytes_of_lines_it_does_not_assert() {
    let source = "# Bytes\r\n\r\n```python\r\na = \"--sql\\nSELECT \\x31::INTEGER\"\r\n# rewritten: stale\r\nb = \"\u{c}\\1 \u{b}\"\r\n```\r\n";
    let path = write("bytes", source);
    run(&path, true);
    let written = std::fs::read_to_string(&path).unwrap();
    for kept in [
        "# Bytes\r\n",
        "\\x31::INTEGER\"\r\n",
        "b = \"\u{c}\\1 \u{b}\"\r\n",
    ] {
        assert!(written.contains(kept), "{kept:?} not in {written:?}");
    }
}
