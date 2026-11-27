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
