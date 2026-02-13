# Future Work

> **Status:** Ideas and deferred features not in the current implementation.

---

## Stub Generation (`.pyi` overload signatures)

Generate `.pyi` stub files with `@overload` signatures for configured database functions. Functions registered via `[[tool.sqlty.functions]]` in `pyproject.toml` — no decorator needed.

### Deferred CLI

```
sqlty generate .           # Analyze source, emit .pyi stubs
sqlty generate --check .   # Exit 1 if stubs are out of date
sqlty clean .              # Delete generated .pyi files
```

---

## Cross-Module Type Resolution

Currently `SQL[UserType]` validation only works when the type is defined in the same file. Future work:

- Follow imports to resolve types defined in other modules
- Support type aliases and re-exports

---

## Typed JSON with `json_build_object`

When `json_build_object` has literal string keys and typed value expressions, sqlty could infer a `TypedDict` instead of plain `dict`:

```sql
SELECT json_build_object('id', id::INTEGER, 'name', name::TEXT)
```
```python
# Future: tuple[UserJson]
# where UserJson = TypedDict('UserJson', id=int, name=str)
```

---

## Configurable SQL Import Alias

The rewriter currently emits `from sqlty import SQL`. Support alternative import styles so the generated code matches the project's conventions:

```toml
[tool.sqlty]
# Default: "from sqlty import SQL"
# import-style controls the import line and how SQL[T](...) is written

# Option 1: aliased import
import-style = "from sqlty import SQL as Q"
# emits: Q[tuple[int, str]]("--sql\n...")

# Option 2: module import
import-style = "import sqlty"
# emits: sqlty.SQL[tuple[int, str]]("--sql\n...")

# Option 3: module alias
import-style = "import sqlty as sy"
# emits: sy.SQL[tuple[int, str]]("--sql\n...")
```

Affects `rewrite`, `check` (import detection), `strip` (import removal), and the magic codec.

---

## Rust Core — User-Type Checking

The Rust `check_source` does not yet perform user-type extraction. Currently handled by a Python wrapper in `core.py` that adds structural checks on top of the Rust backend.
