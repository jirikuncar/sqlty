# Proofs

A Lean 4 model of how sqlty maps SQL types to Python types (`SqlType::python` in
`rust/crates/sqlty-sql/src/analyzer/sql_type.rs`), with proofs about it.

```bash
cd proofs
lake build                                      # checks every proof, builds sqlty-model
cd ..
uv run pytest tests/test_lean_model.py          # the model against sqlty
```

Install Lean with [elan](https://github.com/leanprover/elan); `lean-toolchain` pins the
version.

## What is proved

| Theorem | Statement |
|---------|-----------|
| `python_wf` | For every dialect and every SQL type, nested arrays, rows, structs, maps and unions included, the Python type is none or well-formed: standard library classes only, no empty `tuple[]`, and every union has two or more members spelled differently, none of them a union. |
| `scalarTable_wf` | Every scalar type the Rust code maps, in every dialect, has a well-formed Python type. |
| `integers_are_int` | Integer types load as `int` in every dialect. |
| `decimals_keep_precision` | Exact decimals load as `decimal.Decimal` in every dialect but SQLite and Oracle, whose drivers return floats. |
| `dates_are_datetime`, `sqlite_dates_are_text` | Dates and timestamps load as `datetime` values, except in SQLite, which returns the stored text. |
| `durations_are_timedelta` | Intervals, and MySQL's `TIME`, load as `datetime.timedelta`. |

`WellFormed` (`SqlTy/Python.lean`) is the definition of a valid Python type. The proofs
use only Lean's standard axioms: `#print axioms SqlTy.python_wf` lists `propext`,
`Classical.choice` and `Quot.sound`.

## How the model is tied to the Rust code

A proof about a model says nothing about the code unless the two agree. Two checks hold
them together:

- **The scalar table is generated.** `SqlTy/Table.lean` is what `scalar_python` returns
  for each name it knows, in each dialect. A Rust unit test fails when it is stale:
  `SQLTY_UPDATE_PROOFS=1 cargo test -p sqlty-sql lean_table` rewrites it.
- **The containers are compared.** `SqlTy/Mapping.lean` writes the container rules and
  SQLite's type affinity by hand, after the Rust code. `tests/test_lean_model.py` casts
  to every known scalar type and to random nested types in PostgreSQL and DuckDB, reads
  back the type sqlty resolved, runs it through `sqlty-model`, and compares the Python
  types.

The model starts from sqlty's resolved SQL type, so it does not cover how sqlparser's
`DataType` becomes one. Nor can a proof show what a driver returns at runtime: the driver
value tests (`tests/test_driver_values.py`) do that.

## Layout

| File | Content |
|------|---------|
| `SqlTy/Dialect.lean` | The dialects whose mapping differs |
| `SqlTy/Python.lean` | Python type expressions, their spelling, `WellFormed` and a decision procedure for it |
| `SqlTy/Table.lean` | Generated: the scalar types each dialect maps |
| `SqlTy/Mapping.lean` | SQL types and `python`, the model of the mapping |
| `SqlTy/Theorems.lean` | The proofs |
| `Main.lean` | `sqlty-model`: the mapping of JSON-encoded SQL types, for the conformance test |
