# Type overrides

`[tool.sqlty.types]` sets the Python type a converter above the driver gives a SQL
type or a column.

## By SQL type

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.types]
DATE = "datetime.date"
BOOLEAN = "bool"
NUMERIC = "decimal.Decimal"
```

`schema.sql`:

```sql
CREATE TABLE events (
    day DATE NOT NULL,
    done BOOLEAN,
    price DECIMAL(10, 2) NOT NULL,
    note TEXT NOT NULL
);
```

### Schema columns

```sql
SELECT day, done, price, note FROM events
-- columns: day: datetime.date (DATE)
-- columns: done: bool | None (BOOLEAN)
-- columns: price: decimal.Decimal (DECIMAL(10, 2))
-- columns: note: str (TEXT)
```

### Casts

```sql
SELECT CAST(note AS DATE) AS d FROM events
-- columns: d: datetime.date (DATE)
```

### Expressions that return the column's values

```sql
SELECT MAX(day) AS last FROM events
-- columns: last: datetime.date | None (DATE)
```

### Expressions of another type

```sql
SELECT day || '' AS text FROM events
-- columns: text: str (TEXT)
```

## Arrays of a SQLite ORM

An ORM that stores a SQLite list column as JSON text decodes it to a `list`. With
`ARRAY = "list"` its elements get the type of the spelling, with the elements'
own overrides; any other type is used as written.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.types]
ARRAY = "list"
DATE = "datetime.date"
```

`schema.sql`:

```sql
CREATE TABLE t (a ARRAY, b INT[] NOT NULL, c INTEGER ARRAY, d ARRAY_DATE);
```

```sql
SELECT * FROM t
-- columns: a: list[typing.Any] | None (ARRAY<ANY>)
-- columns: b: list[int] (ARRAY<INT>)
-- columns: c: list[int] | None (ARRAY<INT>)
-- columns: d: list[datetime.date] | None (ARRAY<DATE>)
```

## By column

A key with a dot is `table.column`.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.types]
"events.duration" = "datetime.timedelta"
```

`schema.sql`:

```sql
CREATE TABLE events (duration INTEGER NOT NULL, other INTEGER NOT NULL);
```

```sql
SELECT duration, other FROM events
-- columns: duration: datetime.timedelta (INT)
-- columns: other: int (INT)
```

```python
import datetime

from sqlty import SQL

# error: [narrow-type] "SQL[tuple[int, int]] should be SQL[tuple[datetime.timedelta, int]]"
q = SQL[tuple[int, int]]("--sql\nSELECT duration, other FROM events")
```

## A CHECK constraint no longer narrows

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.types]
"orders.status" = "app.Status"
```

`schema.sql`:

```sql
CREATE TABLE orders (status TEXT NOT NULL CHECK (status IN ('new', 'paid')));
```

```sql
SELECT status FROM orders
-- columns: status: app.Status (TEXT)
```

## Rewriting imports the type's module

`rewrite` and `check --fix` import the module a Python type names, as they import
`datetime` for a date.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.types]
DATE = "datetime.date"
"events.status" = "app.models.Status"
```

`schema.sql`:

```sql
CREATE TABLE events (day DATE NOT NULL, status TEXT NOT NULL);
```

```python
q = "--sql\nSELECT day, status FROM events"
# rewritten-file: import datetime
# rewritten-file: import app.models
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[datetime.date, app.models.Status]]("--sql\nSELECT day, status FROM events")
```
