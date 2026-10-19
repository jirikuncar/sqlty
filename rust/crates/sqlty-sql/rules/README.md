# Inference rules

The result type and nullability of SQL operators and built-in functions are declared here, in
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
| `left-parameter`, `right-parameter` | no | The SQL type (`TEXT`, `ARRAY<TEXT>`, ...) a placeholder on that side gets when the other operand matches its own `left` or `right` |
| `reference` | yes | The documentation the rule follows: product, version, section |
| `reasoning` | yes | Why, in one sentence |

Operand types are the canonical names the analyzer gives (`INT`, `BIGINT`,
`DECIMAL`, `TEXT`, `JSONB`, `TIMESTAMPTZ`, ...; see `SQL_TYPES` in
`compile.rs`), `ARRAY` for any array, or a class: `@integer`, `@numeric`,
`@text`, `@json`, `@address` (`INET`, `CIDR`), `@array`. A string literal is
`TEXT`, a number literal `INT`, `DECIMAL` or `DOUBLE`. In arithmetic, a
placeholder or `NULL` operand takes the other operand's type.

A parameter type types a placeholder operand, as `analyze_params` reports it: in
`doc -> $1` with a `jsonb` `doc`, the rule below makes `$1` text. Only the other
operand's predicate is checked, so one rule can give both sides' parameter types
(`jsonb @> jsonb`). The first rule of the operator that gives a type wins.

```toml
[[rule]]
id = "postgres.json.arrow-jsonb"
operator = "Arrow"
dialects = ["postgres"]
left = ["JSONB"]
type = "JSONB"
nullability = "nullable"
right-parameter = "TEXT"
reference = "PostgreSQL 17 documentation, 9.16.1, Table 9.45 json and jsonb Operators (jsonb -> text → jsonb)"
reasoning = "`->` on a jsonb value gives jsonb; a missing member or element gives NULL."
```

| `type` | Result |
|--------|--------|
| a SQL type | That type |
| `@any` | Any value, `typing.Any` |
| `@unknown` | Untyped |
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

### Function rules

A `[[function]]` table types a call of a built-in function by its first two
arguments:

```toml
[[function]]
id = "postgres.aggregate.sum-integers"
function = "SUM"
dialects = ["postgres"]
argument = ["SMALLINT", "INT"]
type = "BIGINT"
nullability = "aggregate"
reference = "PostgreSQL 17 documentation, 9.21, Table 9.60 General-Purpose Aggregate Functions (sum ( integer ) → bigint)"
reasoning = "Summing small integers widens to bigint so the sum fits."
```

`function` or `functions` name upper-case built-ins, and `argument` is an
operand predicate as above for the first argument; `@known` admits any typed
argument. `arguments` gives one for each of the first two instead, an empty list
admitting any: `arguments = [[], ["JSON", "JSONB"]]` matches a JSON second
argument. `any-argument` matches when at least one argument, at any position, has one of
its types: `any-argument = ["@binary"]` types MySQL `CONCAT('a', bytes_col)` as a
binary string. `nullability` is required:

| `nullability` | NULL when |
|---------------|-----------|
| `non-null` | never, even for NULL arguments (`ROW_NUMBER`, `NUM_NULLS`) |
| `nullable` | always possible, also for non-null arguments (`NULLIF`, `JSON_EXTRACT`) |
| `strict` | an argument is (`LOWER`) |
| `all-arguments` | every argument is (`COALESCE`) |
| `leading(k)` | one of the first `k` arguments is (`CONCAT_WS`, `leading(1)`) |
| `aggregate` | an aggregate's group is empty or all NULL, in `analyzer/nullability.rs` |
| `code` | `analyzer/nullability.rs` decides from the call (`LAG`'s default, `IF`'s branches) |

A dialect that differs from `common/` overrides it with its own rule: `common/` makes
`CONCAT` `non-null`, and `mysql/` makes it `strict`. A `strict` or `aggregate` function
typed by a rule without `argument`, `arguments` or `any-argument` narrows
`WHERE f(col) IS NOT NULL` to `col`.

`type` is a SQL type (`TEXT`, `ARRAY<INT>`, ...) or a computed result. A `(k)` names
argument `k`, counting from 0:

| `type` | Result |
|--------|--------|
| `@any` | Any value, `typing.Any` |
| `@unknown` | Untyped |
| `@void` | `void`: Python `None` |
| `@code` | `SUM` of what no rule types keeps its schema lookup |
| `@argument`, `@argument(k)` | The type of argument 0 or `k` (0 or 1), value set included, as `MIN` and `MAX` return one of the values |
| `@argument-as TYPE` | The first argument's Python type under another SQL type |
| `@argument-type(k)` | The type of argument `k`, whatever value it computes (`ABS`) |
| `@same(k)` | Argument `k`'s type and values (`NULLIF`) |
| `@element(k)` | The element type of array argument `k` (`UNNEST`) |
| `@array(k)` | An array of argument `k`'s type (`ARRAY_AGG`) |
| `@first-known` | The first argument of known type, skipping `NULL` and placeholders (`COALESCE`) |
| `@first-known(k)` | `@first-known` of the arguments from `k` on (`IF(cond, a, b)`) |
| `@str-to-date(k)` | MySQL `STR_TO_DATE`: `DATE`, `TIME` or `DATETIME`, by the parts the literal format at `k` reads |
| `@bound` | The element type of a range argument, else `TEXT` (PostgreSQL `lower`, `upper`) |
| `@mean` | `DOUBLE` for floats, `INTERVAL` for intervals, else `DECIMAL` (`AVG`) |
| `@real` | `DOUBLE` for floats and integers, `DECIMAL` for exact numbers (`SQRT`) |
| `@scaled` | `@real`, but `DECIMAL` with a second argument (`ROUND(x, 2)`) |
| `@trunc(k)` | A timestamp argument's type, `TIMESTAMPTZ` for a date (`DATE_TRUNC`) |
| `@date-add` | MySQL `DATE_ADD(d, INTERVAL n unit)`, typed as `d + INTERVAL n unit` |
| `@timestamp-add` | MySQL `TIMESTAMPADD`: a `DATE` for day units and coarser, else `DATETIME` |
| `@integral` | MySQL `CEIL`, `FLOOR`: an integer for an exact number, `DOUBLE` for an approximate one |
| `@at-zone(k)` | `timezone(zone, ts)`, typed as `ts AT TIME ZONE zone` |
| `@series` | `generate_series`: the common type of the bounds, small integers widened to `INT`, dates to `TIMESTAMPTZ` |
| `@text(k)` | `TEXT`, or argument `k`'s type when it is a binary or bit string |
| `@random` | `DOUBLE`; `random(min, max)` gives the type of the bounds |

A computed result is a variant of `Returns` (`analyzer/functions.rs`), listed in
`FUNCTION_RESULTS` (`compile.rs`). Add one there when no SQL type fits: the rule still
carries the reference and reasoning.

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

`analyzer/operators.rs` keeps one case rules cannot express: MySQL
`date ± INTERVAL n unit`, where the unit decides between `DATE` and `DATETIME`.
It is typed before the rules are tried. Function results that need code are the
computed `type`s above: each rule still names the functions it covers. A built-in no
rule types leaves its column untyped.

## Adding a rule

1. Put it in `rules/common/` when PostgreSQL, MySQL and SQLite all have the
   function or operator and agree on it: the generic dialect and ClickHouse then
   type it too. One only some of them have stays in their directories, with
   `dialects`, so the others still report `unknown-function` for it.
2. Check the `reference` against the vendor documentation of the version the
   tests target (PostgreSQL 17 for the fidelity tests, MySQL 8.4, SQLite 3) and
   cite the section and table. The ISO standard is not freely available, so it
   is not cited.
3. Delete the `match` arm the rule replaces: a behaviour lives in one place.
4. Add a column typed by the rule, with its `-- rules:` assertion, to
   `tests/mdtest/type_inference/rule_coverage.md`;
   `test_every_inference_rule_is_asserted` fails until some assertion names it.
5. Add an mdtest for any case none covers, and run `make test` and
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
