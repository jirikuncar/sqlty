# Inference rules

The result type and nullability of SQL operators are declared here, in
`*.inference.toml` files, instead of in `match` arms of the analyzer. `build.rs`
compiles them into static data and one `match` on the operator
(`$OUT_DIR/inference_rules.rs`), so nothing is read or parsed at run time.

```toml
# rules/postgres/json.inference.toml
[[rule]]
id = "postgres.json.arrow-json"
operator = "Arrow"
dialects = ["postgres"]
left = ["JSON"]
type = "JSON"
nullability = "nullable"
reference = "PostgreSQL 17 documentation, 9.16.1, Table 9.45 json and jsonb Operators (json -> text)"
reasoning = "`->` on a json value gives json; a missing member gives NULL."
```

## Format

| Key | Required | Meaning |
|-----|----------|---------|
| `id` | yes | Unique name, `<directory>.<topic>.<name>` |
| `operator` or `operators` | yes | A sqlparser `BinaryOperator` variant (`LongArrow`, `Plus`, ...), or `Custom:<<=` for an operator sqlparser keeps as text; `operators` lists several that share the rule |
| `dialects` | in a dialect directory | `postgres`, `mysql`, `sqlite`, `clickhouse`; a rule in `common/` has none and applies to every dialect |
| `left`, `right` | no | SQL types the operand may have; absent means any operand |
| `type` | yes | The result: a canonical SQL type (`TEXT`, `JSONB`, ...) or one of the results below |
| `nullability` | no | One of the nullabilities below; `operands` by default |
| `shadows` | no | Ids of later rules of the same tier this one takes precedence over where both match |
| `reference` | yes | The documentation the rule follows: product, version, section |
| `reasoning` | yes | Why, in one sentence |

Operand types are the canonical names the analyzer gives (`INT`, `BIGINT`,
`DECIMAL`, `TEXT`, `JSONB`, `TIMESTAMPTZ`, ...; see `SQL_TYPES` in
`compile.rs`), `ARRAY` for any array, or a class: `@integer`, `@numeric`,
`@text`, `@json`, `@address` (`INET`, `CIDR`), `@array`. A string literal is
`TEXT`, a number literal `INT`, `DECIMAL` or `DOUBLE`. In arithmetic, a
placeholder or `NULL` operand takes the other operand's type.

| `type` | Result |
|--------|--------|
| a SQL type | That type |
| `@any` | Any value, `typing.Any` |
| `@unknown` | Untyped |
| `@code` | Typed by the analyzer's code; the rule gives the nullability only |
| `@left`, `@right` | The type of that operand |
| `@common` | The common type of both operands, as `CASE` and `UNION` unify them |
| `@promote` | The wider number: `SMALLINT` < `INT` < `BIGINT` < `DECIMAL` < `REAL` < `DOUBLE` |
| `@promote-decimal` | `@promote`, and at least `DECIMAL` |
| `@promote-left` | The canonical type of the left number's rank |

| `nullability` | NULL when |
|---------------|-----------|
| `operands` | an operand is |
| `nullable` | always possible, such as a missing JSON member |
| `non-null` | never |
| `short-circuit` | an operand is, the right side read under what the left proves (`AND`, `OR`) |
| `nonzero-divisor` | the divisor is zero, unless it is a number other than zero; else as `operands` |

## How a rule is chosen

For `left op right`, the rules of `op` are tried in order: those of the dialect
directories first, then those of `common/`, each in file order. The first rule
whose dialect and operand types match gives the type and nullability. An
operator with no matching rule is typed by the analyzer's code
(`analyzer/operators.rs`, `analyzer/nullability.rs`).

The operands are typed only when a rule of the operator lists operand types.

## What fails the build

`cargo build` fails, naming the rule id and its file, for:

- malformed TOML or an unknown key;
- an empty `reference` or `reasoning`, or a duplicate `id`;
- an unknown dialect, operand type, result type or nullability;
- an operator that is not a variant name (`rustc` then checks the variant
  exists: the generated `match` names it as `BinaryOperator::<name> { .. }`);
- a rule in `common/` with `dialects`, or a dialect rule without them;
- a rule an earlier rule of the same tier and operator covers entirely: it
  would never apply;
- a partial overlap with a different result or nullability: operands two rules
  of the same tier match, unless an even earlier rule decides them, the earlier
  rule is strictly narrower (a specific case before the general one), or the
  earlier rule lists the later one in `shadows`. Otherwise file order would
  decide silently.

`tests/inference_compiler.rs` covers each case.

## What stays in code

`analyzer/operators.rs` keeps what rules cannot express, each documented where
it lives: MySQL `date ± INTERVAL n unit` (the unit decides between `DATE` and
`DATETIME`), and until their ports the JSON accessors (`->`, `#>`, `#-`) and
date, time and interval arithmetic. Rules with `type = "@code"` give those
operators their nullability.

## Adding a rule

1. Put it in `rules/common/` when every dialect agrees, else in the dialect's
   directory with `dialects`.
2. Check the `reference` against the vendor documentation of the version the
   tests target (PostgreSQL 17 for the fidelity tests, MySQL 8.4, SQLite 3) and
   cite the section and table. The ISO standard is not freely available, so it
   is not cited.
3. Delete the `match` arm the rule replaces: a behaviour lives in one place.
4. Add an mdtest for any case none covers, and run `make test` and
   `make bench-rust`.

## Design notes

These settle the review of the first proposal in
[#350](https://github.com/jirikuncar/sqlty/issues/350).

- **Operators are sqlparser variants.** `->>` is `LongArrow` and `#>>` is
  `HashLongArrow`, not `Custom("->>")`; `rustc` validates the names. Only
  operators sqlparser keeps as text (`<<=`, `>>=`) use `Custom:`, the one
  string comparison at run time.
- **Types are SQL types.** The analyzer works on `Typed` values and derives the
  Python type later, so operand predicates and results are SQL types.
- **Type and nullability are two outputs.** A rule gives both, but nullability
  `operands` defers to the operands' own nullability, which keeps the narrowing
  of `Facts` (CHECK groups, `IS NOT NULL` proofs) in `nullability.rs`. `AND` and
  `OR`, which learn facts for their right side, stay in code.
- **Per-dialect results.** A dialect directory overrides `common/`, so `SUM`
  and the other aggregates can get their PostgreSQL, MySQL and SQLite results
  side by side ([#354](https://github.com/jirikuncar/sqlty/issues/354)).
- **What rules cannot express stays in code**, documented where it lives: a
  result that depends on a literal value (division by a non-zero constant), an
  interval's unit, or facts learned from the left side.
- **No Cargo feature for `--explain`.** Every rule carries its `id`,
  `reference` and `reasoning` as `&'static str`; a trace is recorded only when
  asked for ([#357](https://github.com/jirikuncar/sqlty/issues/357)), so the
  published wheel supports it.
- **Allocation.** Choosing a rule is a `match` on the operator and a scan of a
  static slice; it allocates nothing. The analyzer around it allocates as
  before.
- **Build cost.** `serde` and `toml` are build dependencies of `sqlty-sql` (and
  dev dependencies, for the compiler tests); the code is generated as text, with
  no `quote` or `proc-macro2`.
