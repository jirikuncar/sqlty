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
