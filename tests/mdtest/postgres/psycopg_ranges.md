# Ranges with `driver = "psycopg"`

`schema.sql`:

```sql
CREATE TABLE bookings (id integer PRIMARY KEY, during tstzrange NOT NULL, days daterange, spans int4multirange[]);
```

## Without a driver, a range has no Python type

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL

# error: [unknown-type] "column `bookings.during` has type `TSTZRANGE`, which has no Python type"
q = SQL[tuple[int]]("--sql\nSELECT id, during FROM bookings")
```

## With psycopg

```toml
[tool.sqlty]
schema = "schema.sql"
driver = "psycopg"
```

### Row types are checked against psycopg's classes

```python
from datetime import date, datetime
from typing import TypedDict
from psycopg.types.multirange import Multirange
from psycopg.types.range import Range
from sqlty import SQL

class Booking(TypedDict):
    id: int
    during: Range[datetime]
    days: Range[date] | None
    spans: list[Multirange[int]] | None

class Bounds(TypedDict):
    start: datetime | None
    end: date | None

q = SQL[Booking]("--sql\nSELECT id, during, days, spans FROM bookings")
b = SQL[Bounds]("--sql\nSELECT lower(during) AS start, upper(days) AS end FROM bookings")

class Short(TypedDict):
    days: Range[date]

s = SQL[Short]("--sql\nSELECT days FROM bookings")
# error: [narrow-type] "SQL[tuple[int, Range[date]]] should be SQL[tuple[int, Range[datetime]]]"
t = SQL[tuple[int, Range[date]]]("--sql\nSELECT id, during FROM bookings")
```

`Range` is not read from psycopg's source, so a field annotated with it is
undecided: a wrong element type or a missing `| None` is not reported. An inline
`tuple` is compared as text, so `narrow-type` above names the type the query returns.

### `rewrite` imports psycopg's module

```python
q = "--sql\nSELECT id, during FROM bookings"
# rewritten-file: import datetime
# rewritten-file: import psycopg.types.range
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import NoParams
# rewritten-file: q = SQL[tuple[int, psycopg.types.range.Range[datetime.datetime]], NoParams]("--sql\nSELECT id, during FROM bookings")
```

### `types` replaces an entry

```toml
[tool.sqlty.types]
TSTZRANGE = "str"
```

```sql
SELECT during, days FROM bookings
-- columns: during: str (TSTZRANGE)
-- columns: days: psycopg.types.range.Range[datetime.date] | None (DATERANGE)
```

## Other dialects ignore the driver

```toml
[tool.sqlty]
dialect = "sqlite"
driver = "psycopg"
```

```sql
SELECT CAST(x AS INT4RANGE) AS a
-- columns: a: int (INT4RANGE)
```
