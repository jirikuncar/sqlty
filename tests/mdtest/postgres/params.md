# PostgreSQL query parameters

A placeholder has no type of its own: it takes one from its context. `%s`,
`%(name)s` (psycopg) and `$1` (asyncpg) are read alike.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    age INTEGER,
    born DATE,
    tags TEXT[],
    uid UUID NOT NULL
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    total NUMERIC(10, 2) NOT NULL
);
```

## Comparison

The other side of the comparison types the parameter.

```sql
SELECT id FROM users WHERE name = %s AND age > %s AND born <= %s
-- params: 1: str (TEXT)
-- params: 2: int (INT)
-- params: 3: datetime.date (DATE)
```

### Parameter on the left

```sql
SELECT id FROM users WHERE %s < age
-- params: 1: int (INT)
```

### Native numbering

```sql
SELECT id FROM users WHERE name = $2 AND id = $1
-- params: 1: int (INT)
-- params: 2: str (TEXT)
```

### Named parameters share one entry

```sql
SELECT id FROM users WHERE id = %(id)s OR age > %(id)s OR name = %(name)s
-- params: id: int (INT)
-- params: name: str (TEXT)
```

### Qualified columns, joins and subqueries

```sql
SELECT u.id
FROM users u JOIN orders o ON o.user_id = u.id AND o.total > %s
WHERE u.name = %s
  AND EXISTS (SELECT 1 FROM orders x WHERE x.user_id = u.id AND u.age > %s)
-- params: 1: decimal.Decimal (DECIMAL(10, 2))
-- params: 2: str (TEXT)
-- params: 3: int (INT)
```

### Common table expressions and derived tables

```sql
WITH t AS (SELECT id AS k FROM users WHERE name = %s)
SELECT k FROM t WHERE k = %s AND k IN (SELECT a FROM (SELECT age AS a FROM users WHERE id = %s) s)
-- params: 1: str (TEXT)
-- params: 2: int (INT)
-- params: 3: int (INT)
```

## BETWEEN, IN, LIKE

```sql
SELECT id FROM users
WHERE id BETWEEN %s AND %s AND name IN (%s, 'a') AND name LIKE %s
  AND id IN (SELECT user_id FROM orders WHERE total > %s)
-- params: 1: int (INT)
-- params: 2: int (INT)
-- params: 3: str (TEXT)
-- params: 4: str (TEXT)
-- params: 5: decimal.Decimal (DECIMAL(10, 2))
```

## ANY(%s) is an array

```sql
SELECT id FROM users WHERE id = ANY(%s) AND name = ANY($2)
-- params: 1: list[int] (ARRAY<INT>)
-- params: 2: list[str] (ARRAY<TEXT>)
```

### Searching an array column

```sql
SELECT id FROM users WHERE %s = ANY(tags)
-- params: 1: str (TEXT)
```

## INSERT takes the target column

A `NOT NULL` column makes the parameter non-null; a nullable column admits `None`.

```sql
INSERT INTO users (id, name, age) VALUES (%s, %s, %s)
-- params: 1: int (INT) NOT NULL
-- params: 2: str (TEXT) NOT NULL
-- params: 3: int (INT) NULL
```

### Without a column list

```sql
INSERT INTO orders VALUES (%s, %s, %s), (%s, %s, %s)
-- params: 1: int (INT) NOT NULL
-- params: 2: int (INT) NOT NULL
-- params: 3: decimal.Decimal (DECIMAL(10, 2)) NOT NULL
-- params: 4: int (INT) NOT NULL
-- params: 5: int (INT) NOT NULL
-- params: 6: decimal.Decimal (DECIMAL(10, 2)) NOT NULL
```

### ON CONFLICT DO UPDATE

```sql
INSERT INTO users (id, name) VALUES (%s, %s)
ON CONFLICT (id) DO UPDATE SET name = %s, age = %s
-- params: 1: int (INT) NOT NULL
-- params: 2: str (TEXT) NOT NULL
-- params: 3: str (TEXT) NOT NULL
-- params: 4: int (INT) NULL
```

## UPDATE and DELETE

```sql
UPDATE users SET name = %s, age = %s WHERE id = %s
-- params: 1: str (TEXT) NOT NULL
-- params: 2: int (INT) NULL
-- params: 3: int (INT)
```

```sql
DELETE FROM orders WHERE user_id = %s AND total > %s
-- params: 1: int (INT)
-- params: 2: decimal.Decimal (DECIMAL(10, 2))
```

## Explicit casts

```sql
SELECT %s::int, CAST(%s AS date), %s::text[], %s::uuid
-- params: 1: int (INT)
-- params: 2: datetime.date (DATE)
-- params: 3: list[str] (ARRAY<TEXT>)
-- params: 4: uuid.UUID (UUID)
```

## LIMIT and OFFSET

```sql
SELECT id FROM users LIMIT %s OFFSET %s
-- params: 1: int (BIGINT)
-- params: 2: int (BIGINT)
```

## Function arguments

Known signatures type their arguments; `COALESCE` and friends share one type.

```sql
SELECT lpad(name, %s, %s), coalesce(age, %s) FROM users WHERE lower(name) = lower(%s)
-- params: 1: int (INT)
-- params: 2: str (TEXT)
-- params: 3: int (INT)
-- params: 4: str (TEXT)
```

### SUBSTRING bounds and server functions

```sql
SELECT substr(name, %s), substring(name FROM %s FOR %s), pg_notify(%s, %s), pg_advisory_lock(%s) FROM users
-- params: 1: int (INT)
-- params: 2: int (INT)
-- params: 3: int (INT)
-- params: 4: str (TEXT)
-- params: 5: str (TEXT)
-- params: 6: int (BIGINT)
```

### Functions in the schema

A function the schema declares types its arguments, when the overloads a call
fits agree on them. A `VARIADIC` argument stays untyped.

`schema.sql`:

```sql
CREATE FUNCTION finish_job(job_id BIGINT, new_status TEXT) RETURNS VOID LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION tag(id INTEGER, VARIADIC names TEXT[]) RETURNS INTEGER LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION pick(a INTEGER) RETURNS INTEGER LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION pick(a TEXT) RETURNS INTEGER LANGUAGE sql AS 'SELECT 1';
```

```sql
SELECT finish_job(%(job_id)s, %(status)s), tag(%(id)s, %(name)s), pick(%(a)s)
-- params: job_id: int (BIGINT)
-- params: status: str (TEXT)
-- params: id: int (INT)
-- params: name: None (None)
-- params: a: None (None)
```

## Operators type the other operand

An arithmetic, array, `jsonb` or network operator types a parameter the way
PostgreSQL resolves it: a number like the other number, an interval next to a
timestamp, an array like the other array, a `jsonb` key as text.

`schema.sql`:

```sql
CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    n INTEGER NOT NULL,
    total NUMERIC(10, 2) NOT NULL,
    at TIMESTAMP NOT NULL,
    span INTERVAL NOT NULL,
    note TEXT NOT NULL,
    ids INTEGER[] NOT NULL,
    doc JSONB NOT NULL,
    ip INET NOT NULL
);
```

```sql
SELECT id FROM events WHERE n + %s > 10 AND total * %s > 1 AND at + %s < now() AND span * %s > span
-- params: 1: int (INT)
-- params: 2: decimal.Decimal (DECIMAL(10, 2))
-- params: 3: datetime.timedelta (INTERVAL)
-- params: 4: float (DOUBLE)
```

```sql
SELECT id FROM events WHERE ids @> %s AND doc @> %s AND doc ? %s AND doc ->> %s = %s AND ip << %s AND note = %s || 'x'
-- params: 1: list[int] (ARRAY<INT>)
-- params: 2: typing.Any (JSONB)
-- params: 3: str (TEXT)
-- params: 4: str (TEXT)
-- params: 5: str (TEXT)
-- params: 6: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET)
-- params: 7: str (TEXT)
```

### Rows, CASE and AT TIME ZONE

```sql
SELECT CASE WHEN n > %s THEN %s ELSE note END, at AT TIME ZONE %s FROM events WHERE (id, note) = (%s, %s)
-- params: 1: int (INT)
-- params: 2: str (TEXT)
-- params: 3: str (TEXT)
-- params: 4: int (INT)
-- params: 5: str (TEXT)
```

### INSERT ... SELECT and functions in FROM

```sql
INSERT INTO events (id, n, note) SELECT %s, %s, %s
-- params: 1: int (INT) NOT NULL
-- params: 2: int (INT) NOT NULL
-- params: 3: str (TEXT) NOT NULL
```

```sql
SELECT g FROM generate_series(1, %s) AS g
-- params: 1: int (None)
```

## Unknown context stays untyped

```sql
SELECT %s, id FROM nosuch WHERE x = %s
-- params: 1: None (None)
-- params: 2: None (None)
```

## No parameters

```sql
SELECT id FROM users
-- params:
```

## Unparseable queries have no parameters

```sql
SELEC id FROM users WHERE id = %s
-- params: None
```

## E strings take backslash escapes

```sql
SELECT E'it\'s ?', id FROM users WHERE name = %s
-- params: 1: str (TEXT)
```

## An explicit cast marks the parameter

```sql
SELECT %s::text
-- params: 1: str (TEXT)
```
