# User-Defined Type Checking

Validates that SQL[UserType](...) annotations match the query columns
structurally, supporting TypedDict, NamedTuple, dataclass, and Pydantic
BaseModel types.

## TypedDict — all fields match

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "User"
fields = [["id", "int"], ["name", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error:
```

## TypedDict — field count mismatch

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "User"
fields = [["id", "int"], ["name", "str"], ["email", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error: [column-count-mismatch] "User has 3 fields but query returns 2 columns"
```

## TypedDict — field type mismatch

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "User"
fields = [["id", "str"], ["name", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
```

## TypedDict — field name mismatch

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "UserRow"
fields = [["user_id", "int"], ["username", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error: [column-name-mismatch] "UserRow.user_id doesn't match query column 'id'"
```

## NamedTuple — all fields match

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "UserRow"
fields = [["id", "int"], ["name", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error:
```

## NamedTuple — type mismatch with datetime

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "EventRow"
fields = [["id", "int"], ["created_at", "str"]]
```

```sql
SELECT id::INTEGER, created_at::TIMESTAMP FROM events
```

```python
# error: [column-type-mismatch] "EventRow.created_at is str but query column 'created_at' infers datetime.datetime"
```

## dataclass — all fields match

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "Product"
fields = [["id", "int"], ["name", "str"], ["price", "float"]]
```

```sql
SELECT id::INTEGER, name::TEXT, price::NUMERIC FROM products
```

```python
# error:
```

## dataclass — count mismatch (too few fields)

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "Product"
fields = [["id", "int"]]
```

```sql
SELECT id::INTEGER, name::TEXT, price::NUMERIC FROM products
```

```python
# error: [column-count-mismatch] "Product has 1 fields but query returns 3 columns"
```

## Pydantic BaseModel — all fields match

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "UserModel"
fields = [["id", "int"], ["name", "str"], ["email", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users
```

```python
# error:
```

## Pydantic BaseModel — type mismatch

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "UserModel"
fields = [["id", "int"], ["name", "int"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error: [column-type-mismatch] "UserModel.name is int but query column 'name' infers str"
```

## Single field type — uuid mismatch

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "TokenRow"
fields = [["token_id", "str"]]
```

```sql
SELECT token_id::UUID FROM tokens
```

```python
# error: [column-type-mismatch] "TokenRow.token_id is str but query column 'token_id' infers uuid.UUID"
```

## Multiple mismatches — count takes priority

When both count and type mismatches exist, count is reported first.

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "WrongType"
fields = [["bad_name", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error: [column-count-mismatch] "WrongType has 1 fields but query returns 2 columns"
```

## Schema-based inference with user type

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
[sqlty.annotation]
type = "User"
fields = [["id", "int"], ["name", "str"], ["email", "str"]]
```

```sql
SELECT id, name, email FROM users
```

```python
# error:
```

## Schema-based — type mismatch without explicit casts

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
[sqlty.annotation]
type = "User"
fields = [["id", "str"], ["name", "str"], ["email", "str"]]
```

```sql
SELECT id, name, email FROM users
```

```python
# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
```
