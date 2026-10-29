# Line numbers in large files

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    total NUMERIC(10, 2) NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT NOW()
);
```
```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Diagnostics after many plain strings keep their line

```python
a = "one"
b = "two"
c = "three"
# error: [inline-sql-comment]
q = "--sql SELECT id FROM users"
```

## Columns

A range after `error` asserts the columns a diagnostic underlines: the first, and
the one after the last, counted in characters from 1. Editors underline the same.

### A row type check underlines the row type

```python
from sqlty import SQL

# error[9:19]: [narrow-type]
q = SQL[tuple[int]]("--sql\nSELECT 'é'::TEXT")
```

### Characters before the span count once

```python
from sqlty import SQL

# error[15:25]: [narrow-type]
q = ("é", SQL[tuple[int]]("--sql\nSELECT 1.5::REAL"))
```

### A name check underlines the name

An unknown table, column or function is underlined where it is written, in whichever
string of the query it is.

```python
from sqlty import SQL

# error[+1.22:+1.26]: [unknown-column]
q = SQL[tuple[int]]("--sql\nSELECT "
                    "nope FROM users")
```

### A name in a multi-line query

```python
from sqlty import SQL

# error[+2.6:+2.13]: [unknown-table]
q = SQL[tuple[int]]("""--sql
SELECT id
FROM missing""")
```

### A name written with escapes underlines the escapes

```python
from sqlty import SQL

# error[40:47]: [unknown-column]
q = SQL[tuple[int]]("--sql\nSELECT id, \x6eope FROM users")
```

### Other query checks underline the strings

A cast to an unknown type is about the cast, which has no position of its own.

```python
from sqlty import SQL

# error[21:44]: [unknown-type]
q = SQL[tuple[int]]("--sql\nSELECT 1::mood")
```

### A multi-line call reports on its first line

```python
from sqlty import SQL

# error[+1.5:+1.15]: [narrow-type]
q = SQL[
    tuple[int]
](
    """--sql
SELECT 1.5::REAL"""
)
```

### An unused suppression underlines the comment

```python
x = 1  # sqlty: ignore[unknown-column]  # error[8:39]: [unused-ignore]
```
