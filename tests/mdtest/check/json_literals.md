# JSON shapes and Literal columns

A JSON member built from a column with a value set is still a string or an integer
scalar: the annotation `str` or `int` fits, and a `Literal` is not a mismatch.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid')),
    priority INTEGER NOT NULL CHECK (priority IN (1, 2, 3))
);
```

## Literal-valued members fit their base type

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    status: str
    priority: int


class Row(TypedDict):
    id: int
    doc: Doc


q = SQL[Row]("""--sql
SELECT id, jsonb_build_object('status', status, 'priority', priority) AS doc FROM orders""")
# error:
```

## Constants in a JSON object

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    kind: str
    version: int


class Row(TypedDict):
    doc: Doc


q = SQL[Row]("""--sql
SELECT jsonb_build_object('kind', 'order', 'version', 1) AS doc FROM orders""")
# error:
```

## A wrong scalar is still reported

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    status: int


class Row(TypedDict):
    doc: Doc


# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc.status is int but the query builds a string"
q = SQL[Row]("""--sql
SELECT jsonb_build_object('status', status) AS doc FROM orders""")
```
