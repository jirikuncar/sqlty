# SQLite query parameters

`?`, `?NNN` and `:name` placeholders, as sqlite3 takes them.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    age INTEGER
);
```

## Question marks

```sql
SELECT id FROM users WHERE name = ? AND age > ?
-- params: 1: str (TEXT)
-- params: 2: int (INT)
```

## Numbered question marks

```sql
SELECT id FROM users WHERE age > ?2 AND name = ?1
-- params: 1: str (TEXT)
-- params: 2: int (INT)
```

## Named parameters

```sql
SELECT id FROM users WHERE name = :name OR name = :name || 'x' OR age = :age
-- params: name: str (TEXT)
-- params: age: int (INT)
```

### Casts are not named parameters

```sql
SELECT CAST(:n AS INTEGER) FROM users WHERE id = :id AND age::TEXT = 'x'
-- params: n: int (INT)
-- params: id: int (INT)
```

## INSERT

```sql
INSERT INTO users (id, name, age) VALUES (?, ?, ?)
-- params: 1: int (INT) NOT NULL
-- params: 2: str (TEXT) NOT NULL
-- params: 3: int (INT) NULL
```

## Placeholders in literals and comments are text

```sql
SELECT '?' AS q, ':x' AS c FROM users WHERE id = ? -- ? :y
-- params: 1: int (INT)
```

## IS

```sql
SELECT id FROM users WHERE age IS ? AND name IS NOT :name
-- params: 1: int (INT)
-- params: name: str (TEXT)
```
