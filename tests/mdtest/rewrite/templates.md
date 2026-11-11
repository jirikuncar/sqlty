# t-string queries

A t-string ([PEP 750](https://peps.python.org/pep-0750/)) whose fields are plain
values is a query with parameters: `rewrite` wraps it as `SQLTemplate[T]`, and
`strip` unwraps it.

## Rewrite and strip

```python
user_id = 1
q = t"--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = {user_id} AND name LIKE 'a%'"
# rewritten-file: from sqlty import SQLTemplate
# rewritten-file: user_id = 1
# rewritten-file: q = SQLTemplate[tuple[int, str]](t"--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = {user_id} AND name LIKE 'a%'")
```

```python
from sqlty import SQLTemplate
user_id = 1
q = SQLTemplate[tuple[int]](t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id}")
# stripped: user_id = 1
# stripped: q = t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id}"
```

## Braces, expressions and triple quotes

Doubled braces are text, and a field may hold any expression.

```python
ids = [1, 2]
q = t"""--sql
SELECT id::INTEGER, '{{}}'::JSONB AS empty
FROM users
WHERE id = ANY({ids[:1]}) AND name = {"a" if ids else "b"}"""
# rewritten-file: import typing
# rewritten-file: from sqlty import SQLTemplate
# rewritten-file: ids = [1, 2]
# rewritten-file: q = SQLTemplate[tuple[int, typing.Any]](t"""--sql
# rewritten-file: SELECT id::INTEGER, '{{}}'::JSONB AS empty
# rewritten-file: FROM users
# rewritten-file: WHERE id = ANY({ids[:1]}) AND name = {"a" if ids else "b"}""")
```

## Module import style

```toml
[tool.sqlty]
import-style = "import sqlty"
```

```python
q = t"--sql\nSELECT 1::INTEGER AS n"
r = "--sql\nSELECT 'x'::TEXT AS s"
# rewritten-file: import sqlty
# rewritten-file: q = sqlty.SQLTemplate[tuple[int]](t"--sql\nSELECT 1::INTEGER AS n")
# rewritten-file: r = sqlty.SQL[tuple[str]]("--sql\nSELECT 'x'::TEXT AS s")
```
