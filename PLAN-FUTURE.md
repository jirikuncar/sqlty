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

## ~~Configurable SQL Import Alias~~ — Implemented

> Implemented. See `[tool.sqlty] import-style` in pyproject.toml.

---

## Rust Core — User-Type Checking

The Rust `check_source` does not yet perform user-type extraction. Currently handled by a Python wrapper in `core.py` that adds structural checks on top of the Rust engine.

---

## Engine Gaps

- **SQLite free-form type names.** SQLite accepts any type name and derives an
  affinity (sqlite.org/datatype3.html section 3.1), but `UNSIGNED BIG INT`,
  `VARYING CHARACTER(255)`, `NATIVE CHARACTER(70)` or `anything goes` make
  sqlty skip the table silently. A follow-up could map such columns by the
  affinity rules (contains `INT` -> `int`; `CHAR`/`CLOB`/`TEXT` -> `str`;
  `REAL`/`FLOA`/`DOUB` -> `float`) and warn when a schema table is skipped.
- **Other dialects.** Dialects beyond PostgreSQL, MySQL and SQLite use
  sqlparser-rs's parser with a handful of canonical type names; they have no
  mdtest suite of their own.
- **Two type tables.** SQL -> Python types live in Rust's `SQL_TO_PYTHON_TYPE`
  and, as runtime types for row type checks, in `_type_extractor.py`.
