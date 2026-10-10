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

The Rust `check_source` does not yet perform user-type extraction. Currently handled by a Python wrapper in `core.py` that adds structural checks on top of the Rust backend.

---

## Backend Parity Gaps

Known differences between the Python (sqlglot) and Rust (sqlparser-rs) backends,
and gaps both share:

- **MySQL `INT3 UNSIGNED` / `MIDDLEINT UNSIGNED`.** sqlparser-rs has no `INT3` or
  `MIDDLEINT` keyword, so the trailing `UNSIGNED` is a parse error. For
  `CREATE TABLE t (a INT, b INT3 UNSIGNED)`, Python expands `SELECT *` to
  `a::INT, b::UMEDIUMINT` while Rust skips the whole table (`None`). Pinned by
  the `xfail` test `TestParserParity::test_int3_unsigned_column`; needs an
  upstream sqlparser-rs fix (MySQL documents both as `MEDIUMINT` aliases).
- **MySQL `ZEROFILL`.** `INT UNSIGNED ZEROFILL` columns make both backends skip
  the table.
- **SQLite free-form type names.** SQLite accepts any type name and derives an
  affinity (sqlite.org/datatype3.html section 3.1), but `UNSIGNED BIG INT`,
  `VARYING CHARACTER(255)`, `NATIVE CHARACTER(70)` or `anything goes` make both
  backends skip the table silently. A follow-up could map such columns by the
  affinity rules (contains `INT` -> `int`; `CHAR`/`CLOB`/`TEXT` -> `str`;
  `REAL`/`FLOA`/`DOUB` -> `float`) and warn when a schema table is skipped.
- **Other dialects.** Rust reads any dialect other than PostgreSQL, MySQL and
  SQLite as generic SQL, while Python passes the name to sqlglot. Only the
  three named dialects are kept in parity.
- **Duplicated type tables.** Type aliases and SQL -> Python mappings are kept by
  hand in `SQL_TO_PYTHON_TYPE` (both backends), Rust's `canonical_type` and the
  sqlglot tokenizer overrides in `_sql_analyzer.py`. `test_type_tables_agree`
  and `test_type_names_agree` catch drift; loading them from one shared data
  file would remove it.
