# Quick Start

The simplest way to use SQLTy: write `--sql` queries, run `sqlty rewrite` to add type annotations.

## Step 1: Write queries

```python
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
count_query = "--sql\nSELECT COUNT(*)::INTEGER FROM users"
```

Use `::TYPE` casts to tell SQLTy the return types.

## Step 2: Rewrite

```bash
sqlty rewrite examples/01_quick_start/
```

SQLTy transforms the plain strings into typed `SQL[T](...)` calls:

```python
from sqlty import SQL

user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
count_query = SQL[tuple[int]]("--sql\nSELECT COUNT(*)::INTEGER FROM users")
```

## Step 3: Check

```bash
sqlty check examples/01_quick_start/
```

Verifies that all annotations are correct and up to date.

## Step 4: Strip

```bash
sqlty strip examples/01_quick_start/
```

Removes `SQL[T](...)` wrappers, reverting to plain `--sql` strings.
