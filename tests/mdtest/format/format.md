# Formatting

`sqlty format` lays out query strings with sqruff; only their whitespace changes.

## A wrapped query

```python
from sqlty import SQL

q = SQL[tuple[int]]("""--sql
select id from users where id=%s""")
# formatted: from sqlty import SQL
# formatted:
# formatted: q = SQL[tuple[int]]("""--sql
# formatted: select id from users
# formatted: where id = %s""")
```

## Placeholders

```python
q = """--sql
SELECT a,b FROM t WHERE x = ? AND y=%(y)s AND z LIKE 'a%%'"""
# formatted: q = """--sql
# formatted: SELECT
# formatted:     a,
# formatted:     b
# formatted: FROM t
# formatted: WHERE x = ? AND y = %(y)s AND z LIKE 'a%%'"""
```

## Left alone

A backslash, an f-string or concatenated literals.

```python
q = """--sql
select 'a\tb' as a,b from t"""
r = f"""--sql
select a,b from {t}"""
s = ("""--sql
select a,b """ """from t""")
# formatted: q = """--sql
# formatted: select 'a\tb' as a,b from t"""
# formatted: r = f"""--sql
# formatted: select a,b from {t}"""
# formatted: s = ("""--sql
# formatted: select a,b """ """from t""")
```

## Format fields

The `{name}` fields of a `str.format` or `sql.SQL` template are kept like placeholders,
even where a field stands for a keyword's argument, as `{limit}` does.

```python
q = """--sql
select a,b from {table} where id={id} limit {limit}"""
# formatted: q = """--sql
# formatted: select
# formatted:     a,
# formatted:     b
# formatted: from {table}
# formatted: where id = {id} limit {limit}"""
```

## A query that does not parse

sqruff lays out what it reads and leaves the rest.

```python
q = """--sql
SELEC a,b FROM t"""
# formatted: q = """--sql
# formatted: SELEC a,b FROM t"""
```

## Dialects

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
q = """--sql
select `a`,b from t limit 1"""
# formatted: q = """--sql
# formatted: select
# formatted:     `a`,
# formatted:     b
# formatted: from t limit 1"""
```
