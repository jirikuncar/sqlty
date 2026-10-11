# Query Parameters

DB-API placeholders (`%s`, `%(name)s`) are values of unknown type, so they
don't stop inference; `%%` is a literal `%`. Both backends read them alike in
every dialect.

## Positional

```sql
SELECT id::INTEGER, name::TEXT FROM users WHERE id = %s
-- inferred: tuple[int, str]
```

## Named

```sql
SELECT id::INTEGER FROM users WHERE id = %(id)s AND name LIKE 'a%%'
-- inferred: tuple[int]
```

## Cast placeholder

```sql
SELECT %s::INTEGER, %s
-- inferred: tuple[int, str]
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
q = "--sql\nSELECT CAST(id AS SIGNED) FROM users WHERE id = %s"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT CAST(id AS SIGNED) FROM users WHERE id = %s")
```

## PostgreSQL question marks

`?` placeholders are read as `$1`, `$2`, ... when the query doesn't parse as
written: in PostgreSQL `?` is also a JSONB operator, which keeps its meaning.

```sql
SELECT id::INTEGER FROM users WHERE id = ? AND name = ?
-- inferred: tuple[int]
```

```sql
SELECT id::INTEGER FROM users WHERE tags ? 'admin'
-- inferred: tuple[int]
```

```sql
SELECT id::INTEGER FROM users WHERE tags ?| array['a'] AND id = ?
-- inferred: tuple[int]
```
