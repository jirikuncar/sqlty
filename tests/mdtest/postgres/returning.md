# PostgreSQL RETURNING Clause

```toml
[sqlty]
dialect = "postgres"
```

## INSERT RETURNING single column

```sql
INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')
RETURNING id::INTEGER
```

```python
# inferred: tuple[int]
```

## INSERT RETURNING multiple columns

```sql
INSERT INTO users (name, email)
VALUES ('Alice', 'alice@example.com')
RETURNING id::INTEGER, name::TEXT, created_at::TIMESTAMP
```

```python
# inferred: tuple[int, str, datetime.datetime]
```

## INSERT with multiple VALUES rows

```sql
INSERT INTO users (name, email)
VALUES ('Alice', 'alice@example.com'), ('Bob', 'bob@example.com')
RETURNING id::INTEGER, name::TEXT
```

```python
# inferred: tuple[int, str]
```

## INSERT ON CONFLICT DO UPDATE RETURNING

```sql
INSERT INTO users (email, name)
VALUES ('alice@example.com', 'Alice')
ON CONFLICT (email) DO UPDATE SET name = EXCLUDED.name
RETURNING id::INTEGER, name::TEXT, email::TEXT
```

```python
# inferred: tuple[int, str, str]
```

## INSERT ON CONFLICT DO NOTHING RETURNING

```sql
INSERT INTO users (email, name)
VALUES ('alice@example.com', 'Alice')
ON CONFLICT (email) DO NOTHING
RETURNING id::INTEGER
```

```python
# inferred: tuple[int]
```

## UPDATE RETURNING single column

```sql
UPDATE users SET name = 'Bob' WHERE id = 1
RETURNING id::INTEGER
```

```python
# inferred: tuple[int]
```

## UPDATE RETURNING multiple columns

```sql
UPDATE users SET name = 'Bob', email = 'bob@new.com' WHERE id = 1
RETURNING id::INTEGER, name::TEXT, email::TEXT, updated_at::TIMESTAMP
```

```python
# inferred: tuple[int, str, str, datetime.datetime]
```

## UPDATE with FROM clause RETURNING

```sql
UPDATE orders o
SET status = 'shipped'
FROM shipments s
WHERE o.id = s.order_id
RETURNING o.id::INTEGER, o.status::TEXT, s.tracking_number::TEXT
```

```python
# inferred: tuple[int, str, str]
```

## DELETE RETURNING single column

```sql
DELETE FROM users WHERE active = false
RETURNING id::INTEGER
```

```python
# inferred: tuple[int]
```

## DELETE RETURNING multiple columns

```sql
DELETE FROM users WHERE last_login < NOW() - INTERVAL '1 year'
RETURNING id::INTEGER, name::TEXT, email::TEXT, last_login::TIMESTAMP
```

```python
# inferred: tuple[int, str, str, datetime.datetime]
```

## DML without RETURNING returns None

```sql
INSERT INTO users (name) VALUES ('test')
```

```python
# inferred: None
```

## UPDATE without RETURNING returns None

```sql
UPDATE users SET name = 'test' WHERE id = 1
```

```python
# inferred: None
```

## DELETE without RETURNING returns None

```sql
DELETE FROM users WHERE id = 1
```

```python
# inferred: None
```
