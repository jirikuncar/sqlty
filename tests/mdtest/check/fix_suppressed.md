# check --fix respects suppression comments

`sqlty check --fix` does not apply a fix for a rule to a query whose reported line
carries `# sqlty: ignore` or `# sqlty: ignore[rule]` naming that rule. Other rules on
the line are still fixed.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Suppressed select-star is not expanded

```python
from sqlty import SQL

query = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")  # sqlty: ignore[select-star]
# fixed: from sqlty import SQL
# fixed:
# fixed: query = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")  # sqlty: ignore[select-star]
```

## Unsuppressed select-star is expanded

```python
from sqlty import SQL

query = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: query = SQL[tuple[int, str]]("--sql\nSELECT id::INT, name::TEXT FROM users")
```

## A bare ignore suppresses every fix on its line

```python
from sqlty import SQL

query = SQL[tuple[str]]("--sql\nSELECT * FROM users")  # sqlty: ignore
# fixed: from sqlty import SQL
# fixed:
# fixed: query = SQL[tuple[str]]("--sql\nSELECT * FROM users")  # sqlty: ignore
```

## An unrelated rule is still fixed

```python
from sqlty import SQL

query = SQL[tuple[str]]("--sql\nSELECT * FROM users")  # sqlty: ignore[select-star]
# fixed: from sqlty import SQL
# fixed:
# fixed: query = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")  # sqlty: ignore[select-star]
```

## Suppressed stale-type is not refreshed

```python
from sqlty import SQL

query = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[stale-type]
# fixed: from sqlty import SQL
# fixed:
# fixed: query = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[stale-type]
```

## Suppressed inline-sql-comment is not fixed

```python
query = "--sql SELECT 1"  # sqlty: ignore[inline-sql-comment]
# fixed: query = "--sql SELECT 1"  # sqlty: ignore[inline-sql-comment]
```

## Suppression on the call line covers a split wrapper

```python
from sqlty import SQL

query = SQL[tuple[str]](  # sqlty: ignore[stale-type]
    "--sql\nSELECT id::INTEGER FROM users"
)
# fixed: from sqlty import SQL
# fixed:
# fixed: query = SQL[tuple[str]](  # sqlty: ignore[stale-type]
# fixed:     "--sql\nSELECT id::INTEGER FROM users"
# fixed: )
```

## Suppressed stale-rewrite is not wrapped

The import added for another query shifts the lines; the comment still applies.

```python
other = "--sql\nSELECT id::INTEGER FROM users"
query = "--sql\nSELECT id::INTEGER FROM users"  # sqlty: ignore[stale-rewrite]
# fixed: from sqlty import SQL
# fixed: other = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: query = "--sql\nSELECT id::INTEGER FROM users"  # sqlty: ignore[stale-rewrite]
```

## A suppression on another line does not apply

```python
from sqlty import SQL

query = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
other = 1  # sqlty: ignore
# fixed: from sqlty import SQL
# fixed:
# fixed: query = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: other = 1  # sqlty: ignore
```

## Suppressions keep their line after the import is inserted below a docstring and `__future__`

```python
"""Queries."""
from __future__ import annotations

other = "--sql\nSELECT id::INTEGER FROM users"
query = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")  # sqlty: ignore[select-star]
# fixed: """Queries."""
# fixed: from __future__ import annotations
# fixed: from sqlty import SQL
# fixed:
# fixed: other = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: query = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")  # sqlty: ignore[select-star]
```
