# Check and User-Defined Types

`sqlty check` validates annotations against queries. Use `sqlty rewrite` to generate tuple types, then optionally replace with your own types.

## Workflow

### 1. Generate tuple types

```bash
sqlty rewrite .
```

Produces `SQL[tuple[int, str]](...)` annotations.

### 2. Replace with named types (optional)

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    email: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")
```

### 3. Check

```bash
sqlty check .
```

`sqlty check` validates:
- **Field count**: `User` has 3 fields and the query returns 3 columns
- **Field types**: `id: int` matches `INTEGER`, `name: str` matches `TEXT`, etc.
- **Field names**: `id` matches column `id`, `name` matches column `name`, etc.

### Supported type forms

All of these work with `SQL[T]` and `sqlty check`:

- `TypedDict`
- `NamedTuple`
- `@dataclass`
- Pydantic `BaseModel`

### Check vs Rewrite

| Command | Purpose |
|---------|---------|
| `sqlty rewrite .` | Transform `--sql` strings into `SQL[T](...)` |
| `sqlty check .` | Validate existing annotations are correct |
| `sqlty strip .` | Remove `SQL[T](...)`, revert to plain strings |
| `sqlty rewrite --check .` | Exit 1 if files need rewriting (CI mode) |
