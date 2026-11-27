# Go to definition

`sqlty server` answers `textDocument/definition` for a word of a query string: a table, view
or column goes to the schema statement that last defined it, an alias or CTE to where the
query names it. A string has definitions exactly when `sqlty check` checks it. The
`<CURSOR>` marks the word; `# definition:` holds `file:line text` for the schema, `query text`
for the query, `None` for none.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, total INTEGER);
CREATE VIEW user_names AS SELECT id, name FROM users;
```

`renames.sql`:

```sql
ALTER TABLE users RENAME COLUMN name TO full_name;
ALTER TABLE users ADD COLUMN email TEXT;
```

```toml
[tool.sqlty]
schema = ["schema.sql", "renames.sql"]
```

## Table

```python
q = "--sql\nSELECT 1 FROM ord<CURSOR>ers"
# definition: schema.sql:2 orders
```

## View

```python
q = "--sql\nSELECT 1 FROM user_na<CURSOR>mes"
# definition: schema.sql:3 user_names
```

## Column

```python
q = "--sql\nSELECT o.tot<CURSOR>al FROM orders o"
# definition: schema.sql:2 total
```

## Renamed column

A column goes to the statement that gave it its current name.

```python
q = "--sql\nSELECT full_n<CURSOR>ame FROM users"
# definition: renames.sql:1 full_name
```

## Added column

```python
q = "--sql\nSELECT ema<CURSOR>il FROM users"
# definition: renames.sql:2 email
```

## Alias

```python
q = "--sql\nSELECT u<CURSOR>.id FROM users u"
# definition: query u
```

## CTE

```python
q = "--sql\nWITH big AS (SELECT 1 AS n) SELECT 1 FROM b<CURSOR>ig"
# definition: query big
```

## Unknown table

```python
q = "--sql\nSELECT 1 FROM nowh<CURSOR>ere"
# definition: None
```

## System catalog

```python
q = "--sql\nSELECT 1 FROM pg_cl<CURSOR>ass"
# definition: None
```

## Unchecked string

```python
q = "SELECT 1 FROM ord<CURSOR>ers"
# definition: None
```

## Output columns

A column of a CTE or a subquery goes to the select item that gives it, at any depth. An `ORDER BY` or `GROUP BY` alias or position goes to its select item, or on to the
column a plain item reads; a set operation's are those of its first branch.

### CTE column

```python
q = "--sql\nWITH big AS (SELECT user_id, SUM(total) AS spent FROM orders GROUP BY user_id) SELECT spe<CURSOR>nt FROM big"
# definition: query SUM(total) AS spent
```

### Position

```python
q = "--sql\nWITH big AS (SELECT user_id, SUM(total) AS spent FROM orders GROUP BY user_id) SELECT user_id, spent FROM big ORDER BY <CURSOR>2"
# definition: query SUM(total) AS spent
```

### Nested subqueries

```python
q = "--sql\nSELECT t.n FROM (SELECT i.<CURSOR>n FROM (SELECT total AS n FROM orders) i) t"
# definition: query total AS n
```

### Alias

```python
q = "--sql\nSELECT COUNT(*) AS n FROM orders GROUP BY user_id ORDER BY <CURSOR>n"
# definition: query COUNT(*) AS n
```

### Set operation

```python
q = "--sql\nSELECT id FROM users UNION SELECT id FROM orders ORDER BY <CURSOR>1"
# definition: schema.sql:1 id
```

### Out of range

```python
q = "--sql\nSELECT id FROM users ORDER BY <CURSOR>3"
# definition: None
```
