# MySQL query parameters

MySQL drivers take `%s` and `%(name)s`; the protocol itself takes `?`.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(50) NOT NULL, age INT);
```

## Format and pyformat

```sql
SELECT id FROM users WHERE name = %s AND age > %(min)s
-- params: 1: str (VARCHAR(50))
-- params: min: int (INT)
```

## Question marks

```sql
INSERT INTO users (id, name, age) VALUES (?, ?, ?)
-- params: 1: int (INT) NOT NULL
-- params: 2: str (VARCHAR(50)) NOT NULL
-- params: 3: int (INT) NULL
```
