# Why SQLTy?

## The problem

Raw SQL in Python is untyped. The driver returns rows of `Any`, and nothing
connects the columns a query selects to the code that reads them:

```python
row = cur.execute("SELECT id, name, created_at FROM users WHERE id = %s", (user_id,)).fetchone()
row[2].isoformat()  # is column 2 a datetime? Only the SQL knows.
```

When a column is renamed, retyped or reordered, every query and every
consumer has to be found by hand. Mistakes surface at runtime.

## What sqlty does

sqlty reads the SQL, infers the row type from casts or a schema, and puts it
next to the query as `SQL[T]`, a `str` subclass:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# rewritten: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

The query stays plain SQL that you can read, copy into a console and run.
Type checkers see `T`; `sqlty check` keeps `T` and the SQL in sync, and
validates your own row types against the columns.

## When to use something else

sqlty fits raw SQL written as string literals, with casts or a schema dump
in the repository; [Use cases](use-cases.md) lists the typical situations.
Use something else for:

- **An ORM** (SQLAlchemy, Django): use its typing support.
- **Dynamic SQL** built at runtime: sqlty only reads string literals.
- **Runtime validation**: sqlty is static analysis. Use Pydantic to validate
  rows as they arrive.

## How it compares

| Approach | Trade-off |
|----------|-----------|
| ORM types | Tied to the ORM and its query API |
| Runtime validation (Pydantic) | Catches mismatches only when the code runs |
| Hand-written annotations | Drift from the SQL silently |
| sqlty | Needs casts or a schema; queries must be literals |
