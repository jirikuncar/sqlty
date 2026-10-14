# Check the number of query parameters

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Positional placeholders

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

execute("DELETE FROM users WHERE id = %s AND name = %s", (1, "a"))
# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
execute("DELETE FROM users WHERE id = %s AND name = %s", (1,))
# error: [param-count-mismatch] "query has 1 parameter, the call passes 2"
execute("DELETE FROM users WHERE id = %s", (1, 2))
# error: [param-count-mismatch] "query has 0 parameters, the call passes 1"
execute("DELETE FROM users", (1,))
```

## Numbered placeholders

`$2` alone means two parameters, as the server counts them.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

async def fetch(query: Annotated[LiteralString, SQL], *args: object) -> list[object]: ...

async def main() -> None:
    await fetch("SELECT id FROM users WHERE id = $1 OR age = $1", 1)
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await fetch("SELECT id FROM users WHERE id = $2", 1)
    # error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
    await fetch("SELECT id FROM users WHERE id = $1")
```

## Named placeholders

A name used twice is one parameter.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: dict[str, object], /) -> None: ...

execute("DELETE FROM users WHERE id = %(id)s OR age = %(id)s", {"id": 1})
# error: [param-count-mismatch] "call is missing parameter 'name'"
execute("DELETE FROM users WHERE id = %(id)s AND name = %(name)s", {"id": 1})
execute("DELETE FROM users WHERE id = %(id)s", {"id": 1, "age": 3})
# error: [param-count-mismatch] "call is missing parameter 'id'"
execute("DELETE FROM users WHERE id = %(id)s", {"ident": 1})
```

## Positional arguments for named placeholders are not compared

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

execute("DELETE FROM users WHERE id = %(id)s", (1, 2))
# error:
```

## Unspelled arguments are not compared

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

def f(args, *rest):
    execute("DELETE FROM users WHERE id = %s", args)
    execute("DELETE FROM users WHERE id = %s AND age = %s", (*rest,))
# error:
```

## Rule configuration

```toml
[tool.sqlty.rules]
param-count-mismatch = "warn"
```

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

# error: [param-count-mismatch] "query has 1 parameter, the call passes 2"
execute("DELETE FROM users WHERE id = %s", (1, 2))
```
