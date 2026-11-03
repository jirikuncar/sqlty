# Highlighting

`core.highlight_source` lists the SQL tokens to color in a file, and `sqlty server` sends
them as semantic tokens. A string is highlighted exactly when `sqlty check` checks it.
`# highlighted:` lists `kind text` per token, a line each.

## Marked string

The marker is a comment. Names stay uncolored, even those sqlparser knows as keywords,
such as `name` and `status`.

```python
q = "--sql\nSELECT name, status FROM users WHERE id = $1 AND total > 1.5"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator =
# highlighted: parameter $1
# highlighted: keyword AND
# highlighted: operator >
# highlighted: number 1.5
```

## Wrapper, adjacent literals and escapes

A token split across literals is colored in each; an escape sequence is colored whole.

```python
from sqlty import SQL

q = SQL[tuple[str]](
    "--sql\n"
    "SELECT 'a\tb' "
    'FR' "OM users"
)
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: string 'a\tb'
# highlighted: keyword FR
# highlighted: keyword OM
```

## Triple-quoted string

A token over several lines, such as a block comment, is colored on each.

```python
q = """-- sql
SELECT id /* the
key */ FROM users"""
# highlighted: comment -- sql
# highlighted: keyword SELECT
# highlighted: comment /* the
# highlighted: comment key */
# highlighted: keyword FROM
```

## Annotated helper

A string passed to a parameter marked `Annotated[LiteralString, SQL]` is highlighted, with
no `--sql` marker. So are psycopg's `%s` and `%(name)s`.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T]) -> list[T]: ...

fetch_all("SELECT id FROM users WHERE name = %(name)s OR id = %s", tuple[int])
# highlighted: keyword SELECT
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator =
# highlighted: parameter %(name)s
# highlighted: keyword OR
# highlighted: operator =
# highlighted: parameter %s
```

## Configured function

```toml
[[tool.sqlty.functions]]
import-path = "pkg.db.fetch_all"
query-param = "query"
```

`pkg/db.py`:

```python
from typing import LiteralString

def fetch_all(query: LiteralString) -> list[tuple[int]]: ...
```

```python
from pkg.db import fetch_all

fetch_all(query="SELECT 1")
# highlighted: keyword SELECT
# highlighted: number 1
```

## Dialect

MySQL reads `#` as a comment and `?` as a placeholder.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
q = "--sql\nSELECT id FROM users WHERE id = ? # one"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator =
# highlighted: parameter ?
# highlighted: comment # one
```

## Unchecked strings

A string without a marker, an f-string, a bytes string and a call to an unmarked function
stay Python strings.

```python
from sqlty import SQL

def run(query: str) -> None: ...

a = "SELECT 1"
b = f"--sql\nSELECT {a}"
c = b"--sql\nSELECT 1"
run("SELECT 1")
# highlighted: None
```
