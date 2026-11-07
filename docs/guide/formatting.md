---
description: How to format SQL query strings inside Python files with sqlty format, using sqruff layout rules in your project's dialect, in place or as a diff.
---

# Formatting queries

`sqlty format` lays out the query strings `sqlty check` reads with the layout rules of
[sqruff](https://github.com/quarylabs/sqruff), in the project's dialect:

```bash
sqlty format .           # format in place
sqlty format --diff .    # show what would change
sqlty format --check .   # exit 1 when a file would change, for CI
```

```python
q = """--sql
select id,name from users u join orders o on o.user_id=u.id where u.id=%s
"""
# formatted: q = """--sql
# formatted: select
# formatted:     id,
# formatted:     name
# formatted: from users u
# formatted: join orders o on o.user_id = u.id
# formatted: where u.id = %s
# formatted: """
```

The formatter only changes whitespace, so the query means the same. Keywords keep
their case, and placeholders such as `%s`, `%(name)s`, `?` and the `{name}` fields of
`sql.SQL` templates stay where they are.

## What is formatted

A query is formatted when it is one plain triple-quoted literal without a backslash.
Its text is then the source text, so the layout needs no escape sequences. The
`--sql` line, the indentation of the first line of SQL and the whitespace before the
closing quotes are kept:

```python
def find(user_id: int) -> str:
    return '''--sql
        SELECT a , b FROM t WHERE x = %(x)s'''
# formatted: def find(user_id: int) -> str:
# formatted:     return '''--sql
# formatted:         SELECT
# formatted:             a,
# formatted:             b
# formatted:         FROM t
# formatted:         WHERE x = %(x)s'''
```

Other strings are left alone:

| Query | Why |
|-------|-----|
| `"--sql\nSELECT a, b FROM t"` | A single-quoted string would need `\n` escapes for every new line |
| `"""--sql\nSELECT 'a\tb'"""` | A backslash: the text in the source is not the query |
| `f"""--sql ..."""`, `r"""..."""`, `"a" "b"` | f-, raw and bytes strings, and literals joined across lines |

```python
q = "--sql\nselect a,b from t"
# formatted: q = "--sql\nselect a,b from t"
```

!!! warning "Formatting changes the strings"
    `rewrite` and `strip` never edit a string, but `format` does: it rewrites the
    whitespace inside your queries. Run it before `rewrite --check` in CI, as you run
    a code formatter before a linter.

## In the editor

`sqlty server` offers `sqlty format` as the code action `source.format.sqlty`, so an
editor can run it on save next to your Python formatter. See
[Format on save](../recipes/vscode.md#format-on-save).

## Lint

The [`sqruff`](../reference/rules.md#sqruff) rule reports what sqruff's core
lint rules, all but layout, find in your queries. It is off by default:

```toml
[tool.sqlty.rules]
sqruff = "warn"
```

## Configuration

sqruff reads no configuration file here, so the layout is the same on every machine.
The dialect is the project's `dialect`; one sqruff has no grammar for is formatted as
ANSI SQL.
