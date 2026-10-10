# Motivation

## When to Use SQLTy

**You write raw SQL in Python and want type safety.**

If you use an ORM (SQLAlchemy, Django ORM), use its type system instead. SQLTy is for projects that use raw SQL strings — whether by choice, for performance, or because of legacy code.

## The Problem

Raw SQL queries in Python are untyped:

```python
row = db.fetchone("SELECT id, name, created_at FROM users WHERE id = %s", (user_id,))
# row is tuple[Any, ...] — no type information
print(row[0])  # int? str? datetime? No way to know without reading the SQL
```

Bugs hide in plain sight:

```python
# Renamed 'name' to 'display_name' in the table, but forgot this query
row = db.fetchone("SELECT id, name FROM users")
# Runtime error — no static analysis caught it
```

## How SQLTy Helps

### 1. Type inference from SQL

SQLTy parses your SQL and infers the Python types. The `# rewritten:` line shows the code after `sqlty rewrite .`:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# rewritten: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### 2. Structural validation of user types

Define your own types and sqlty validates them against the query:

```python
class User(TypedDict):
    id: int
    name: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# sqlty check validates field count, types, and names
```

### 3. Lint checks catch drift

When queries change but annotations don't, `sqlty check` catches it:

```
error[stale-type]: SQL[tuple[int, str]] should be SQL[tuple[int, str, str]]
error[column-type-mismatch]: User.id is str but query column 'id' infers int
```

## Trade-offs

### Good fit

- Raw SQL queries (not ORM)
- Codebase with many queries that drift over time
- Team that values static analysis
- Can add `::TYPE` casts to SQL or provide a schema dump

### Use something else

- **ORM users**: Use SQLAlchemy/Django type plugins
- **Small projects**: Manual annotations may be simpler
- **No SQL control**: SQLTy needs type casts or a schema file for inference
- **Runtime validation needed**: SQLTy is static analysis, not runtime checks (use Pydantic for that)

## How It Compares

| Approach | When | Trade-off |
|----------|------|-----------|
| **ORM type plugins** | Using SQLAlchemy/Django | Tied to ORM |
| **Pydantic validation** | Need runtime checks | Runtime overhead |
| **Manual annotations** | Small codebase | Drifts from reality |
| **SQLTy** | Raw SQL at scale | Needs type casts or schema |
