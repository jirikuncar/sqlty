# Query Placeholders

psycopg's `%s` and `%(name)s` placeholders, and PostgreSQL's `$1`, do not
change the row type. A `%s` inside a string literal is plain text.

## Positional placeholder

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
q = "--sql\nSELECT id, name FROM users WHERE id = %s"
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users WHERE id = %s")
```

## Named placeholder

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
q = "--sql\nSELECT id, name FROM users WHERE id = %(id)s"
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users WHERE id = %(id)s")
```

## Mixed placeholders

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
q = "--sql\nSELECT id FROM users WHERE id = %s AND name = %(name)s OR id = $1 LIMIT %s"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT id FROM users WHERE id = %s AND name = %(name)s OR id = $1 LIMIT %s")
```

## Placeholders in DML

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
q = "--sql\nINSERT INTO users (id, name) VALUES (%s, %(name)s) RETURNING id, name"
# rewritten: q = SQL[tuple[int, str]]("--sql\nINSERT INTO users (id, name) VALUES (%s, %(name)s) RETURNING id, name")
```

## Placeholder text in a string literal

```python
q = "--sql\nSELECT name::TEXT FROM users WHERE name LIKE '%s%%'"
# rewritten: q = SQL[tuple[str]]("--sql\nSELECT name::TEXT FROM users WHERE name LIKE '%s%%'")
```

## Stale type with placeholders

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
from sqlty import SQL

a = SQL[tuple[str, str]]("--sql\nSELECT id, name FROM users WHERE id = %s")
b = SQL[tuple[str]]("--sql\nSELECT id FROM users WHERE name = %(name)s")
# check: [stale-type] "SQL[tuple[str, str]] should be SQL[tuple[int, str]]"
# check: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
```
