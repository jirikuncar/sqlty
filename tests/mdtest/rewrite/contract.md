# Rewrite Contract

## `rewrite_source`

### Adds datetime import

```python
q = "--sql\nSELECT ts::TIMESTAMP"
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT ts::TIMESTAMP")
```

### Adds uuid import

```python
q = "--sql\nSELECT uid::UUID"
# rewritten-file: import uuid
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT uid::UUID")
```

### Non sql unchanged

```python
x = 42
name = "hello"
# rewritten-file: x = 42
# rewritten-file: name = "hello"
```

### Already wrapped unchanged

```python
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

### Single quotes

```python
q = '--sql\nSELECT 1::INTEGER'
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]('--sql\nSELECT 1::INTEGER')
```

### Preserves surrounding code

```python
x = 1
q = "--sql\nSELECT 1::INTEGER"
y = 2
# rewritten-file: from sqlty import SQL
# rewritten-file: x = 1
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten-file: y = 2
```

### Multiple queries

```python
a = "--sql\nSELECT 1::INTEGER"
b = "--sql\nSELECT id::TEXT FROM t"
# rewritten-file: from sqlty import SQL
# rewritten-file: a = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten-file: b = SQL[tuple[str]]("--sql\nSELECT id::TEXT FROM t")
```

### Returns str

```python
x = 1
# rewritten-file: x = 1
```

### Invalid sql not wrapped

```python
q = "--sql\nNOT VALID SQL"
# rewritten-file: q = "--sql\nNOT VALID SQL"
```

### Empty query not wrapped

```python
q = "--sql"
# rewritten-file: q = "--sql"
```

### Whitespace only not wrapped

```python
q = "--sql   "
# rewritten-file: q = "--sql   "
```

### Update without returning not wrapped

```python
q = "--sql\nUPDATE users SET name = 'x'"
# rewritten-file: q = "--sql\nUPDATE users SET name = 'x'"
```

### Insert without returning not wrapped

```python
q = "--sql\nINSERT INTO users (name) VALUES ('x')"
# rewritten-file: q = "--sql\nINSERT INTO users (name) VALUES ('x')"
```

### Delete without returning not wrapped

```python
q = "--sql\nDELETE FROM users"
# rewritten-file: q = "--sql\nDELETE FROM users"
```

### Create table not wrapped

```python
q = "--sql\nCREATE TABLE t (id INT)"
# rewritten-file: q = "--sql\nCREATE TABLE t (id INT)"
```

### Drop table not wrapped

```python
q = "--sql\nDROP TABLE users"
# rewritten-file: q = "--sql\nDROP TABLE users"
```

### Begin not wrapped

```python
q = "--sql\nBEGIN"
# rewritten-file: q = "--sql\nBEGIN"
```

### Dml with returning is wrapped

```python
q = "--sql\nUPDATE users SET x=1 RETURNING id::INTEGER"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nUPDATE users SET x=1 RETURNING id::INTEGER")
```

### SELECT without casts still wraps — type defaults to tuple[str, ...]

```python
q = "--sql\nSELECT id, name FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[str, str]]("--sql\nSELECT id, name FROM users")
```

### --sql SELECT ... with space is a SQL comment — parser sees empty query

```python
q = "--sql SELECT 1::INTEGER"
# rewritten-file: q = "--sql SELECT 1::INTEGER"
```

## Untyped sentinel

### Skipped by default

```python
q = "--sql\nVALUES (1, 2)"
# rewritten-file: q = "--sql\nVALUES (1, 2)"
```

### Skips statements without rows

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nDELETE FROM users"
# rewritten-file: q = "--sql\nDELETE FROM users"
```

### Skips statements without rows: `q = "--sql\nINSERT INTO users (id) VALUES (1)"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nINSERT INTO users (id) VALUES (1)"
# rewritten-file: q = "--sql\nINSERT INTO users (id) VALUES (1)"
```

### Skips statements without rows: `q = "--sql\nUPDATE users SET id = 1"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nUPDATE users SET id = 1"
# rewritten-file: q = "--sql\nUPDATE users SET id = 1"
```

### Skips statements without rows: `q = "--sql\nCREATE TABLE users (id INTEGER)"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nCREATE TABLE users (id INTEGER)"
# rewritten-file: q = "--sql\nCREATE TABLE users (id INTEGER)"
```

### Skips statements without rows: `q = "--sql\nDROP TABLE users"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nDROP TABLE users"
# rewritten-file: q = "--sql\nDROP TABLE users"
```

### Skips statements without rows: `q = "--sql\nBEGIN"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nBEGIN"
# rewritten-file: q = "--sql\nBEGIN"
```

### Skips statements without rows: `q = "--sql\nCOMMIT"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nCOMMIT"
# rewritten-file: q = "--sql\nCOMMIT"
```

### Skips statements without rows: `q = "--sql\n"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\n"
# rewritten-file: q = "--sql\n"
```

### Skips statements without rows: `q = "--sql VALUES (1, 2)"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql VALUES (1, 2)"
# rewritten-file: q = "--sql VALUES (1, 2)"
```

### Skips statements without rows: `q = "--sql SELECT FROM users"`

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql SELECT FROM users"
# rewritten-file: q = "--sql SELECT FROM users"
```

### Inferable queries unaffected

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nSELECT 1::INTEGER"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Regression cases

### Rewrite skips prefixed strings

```python
a = f"--sql\nSELECT {x}::INTEGER"
b = r"--sql\nSELECT 1::INTEGER"
# rewritten-file: a = f"--sql\nSELECT {x}::INTEGER"
# rewritten-file: b = r"--sql\nSELECT 1::INTEGER"
```

## Rewrite_source returns unchanged source when no --sql strings

```python
q = "SELECT 1"
# rewritten-file: q = "SELECT 1"
```
