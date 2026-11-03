# A host-agnostic engine

> **Status:** stage 1 (no I/O in the engine crates) merged in
> [#862](https://github.com/jirikuncar/sqlty/pull/862). Stages 2 and 3 need the driver
> profile of [#857](https://github.com/jirikuncar/sqlty/issues/857); stage 5 needs a
> decision on the crate name.

Tracks [#856](https://github.com/jirikuncar/sqlty/issues/856), part of the epic
[#855](https://github.com/jirikuncar/sqlty/issues/855): make `sqlty-sql` an engine that a
TypeScript or Rust host can depend on, with no change to Python behavior.

## Stages

| Stage | Content | State |
|-------|---------|-------|
| 1 | Guard: no file, process or network access in `sqlty-sql` and `sqlty-python` | merged, [#862](https://github.com/jirikuncar/sqlty/pull/862) |
| 2 | Host-neutral type IR (SQL type, nullability, JSON shape, literal values); `SqlType::python` becomes the Python renderer | needs the driver profile of #857 |
| 3 | TypeScript and Rust renderers of the IR, with mdtest-style tables of every scalar | not started |
| 4 | Row-shape comparison in the engine: the host supplies declared fields, the engine reports `column-count-mismatch`, `nullable-mismatch` and the rest | not started |
| 5 | Stable, documented public API; publish to crates.io with an MSRV | needs a decision: crate name |

## Stage 1

- Schema files are read by the host (`src/sqlty/_config.py`), never by the engine:
  `load_schema_with_skipped` takes the SQL text. So the guard needs no exception for
  schema loading.
- `rust/crates/sqlty-sql/clippy.toml` lists the `std::fs`, `std::path::Path` metadata,
  `std::process`, `std::net` and standard stream calls as disallowed; `sqlty-python`
  links the same file. `cargo clippy --all-targets -D warnings` (prek and CI) fails on
  any use, however it is imported.
- The three unit tests that read files or run `cargo metadata` carry
  `#[expect(clippy::disallowed_methods, ...)]`. `expect` fails once the test stops
  needing it.
- `build.rs` reads the inference rules at compile time, never at run time, and says so
  with a crate-level `#![expect(clippy::disallowed_methods, ...)]`.
- `sqlty-lsp` and `sqlty-ext` talk to the editor and to Python, so they stay outside
  the guard.

## Stage 2

The analyzer renders Python strings directly (`SqlType::python`, `scalar_python`), and
`proofs/SqlTy/Table.lean` is generated from `scalar_python`. The Python string is also
the analyzer's working type: about 190 places build one, and operator typing reads them
back (`operators.rs` splits `int | float` to decide whether a sum is numeric).

Most of the IR exists already: every column carries its canonical SQL type
(`Column::sql_type`), which `SqlType::parse` reads into arrays, rows, structs and maps,
with nullability and literal sets beside it. What a host type is depends on the driver,
not only on the SQL type: node-postgres returns `BIGINT` and `NUMERIC` as `string`,
`sqlx` decodes `BIGINT` as `i64`. So a renderer is a function of the `SqlType`, the
dialect and a driver profile, and the profile is a configuration key of #857. Settle
the profile there first; then a TypeScript or Rust renderer reads `SqlType` and needs no
change to how the analyzer types operators.

## Stage 4

`src/sqlty/_check.py` compares inferred columns with the row type the extractor read
(TypedDict, dataclass, ...). The comparison is host-neutral once the host hands over
field names, types and optionality; the extraction stays in each host.

## Stage 5

The workspace is `publish = false`. Publishing needs a crate name (`sqlty-sql` or
`sqlty`), an API reviewed for semver, and owners on crates.io; decide with the
maintainer before opening it.
