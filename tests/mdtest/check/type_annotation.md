# Type Annotation Checks

## Column count mismatch — too few fields

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "User"
fields = [["id", "int"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
```

## Column count mismatch — too many fields

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

## Column type mismatch

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

## Column name mismatch

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "User"
fields = [["user_id", "int"], ["username", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error: [column-name-mismatch] "User.user_id doesn't match query column 'id'"
```

## All fields match — no error

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

## Count mismatch rule disabled — no error

```toml
[sqlty]
dialect = "postgres"
[sqlty.rules]
column-count-mismatch = "ignore"
[sqlty.annotation]
type = "User"
fields = [["id", "int"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error:
```

## Type mismatch rule disabled — no error

```toml
[sqlty]
dialect = "postgres"
[sqlty.rules]
column-type-mismatch = "ignore"
[sqlty.annotation]
type = "User"
fields = [["id", "str"], ["name", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error:
```

## Name mismatch when count also mismatches — count reported first

Count mismatch takes priority over name/type checks.

```toml
[sqlty]
dialect = "postgres"
[sqlty.rules]
column-count-mismatch = "error"
column-type-mismatch = "error"
[sqlty.annotation]
type = "Row"
fields = [["wrong_name", "str"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error: [column-count-mismatch] "Row has 1 fields but query returns 2 columns"
```
