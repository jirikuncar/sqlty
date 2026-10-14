# Benchmarks

Two scripts; both need the release extension:
`uv run maturin develop --release --manifest-path rust/crates/sqlty-ext/Cargo.toml`.

| Script | What it measures | Make target |
|--------|------------------|-------------|
| `bench_core.py` | `sqlty.core` calls on small inputs (µs per call) | `make bench` |
| `bench_project.py` | A generated large project: CLI wall time and peak RSS | `make bench-project`, `make bench-ci`, `make bench-baseline` |

## The synthetic project

`synth.py` writes a deterministic project (seeded) into a temp directory at run
time; nothing is committed. `full` is 10,000 Python files, 3,000 queries and a
500-table schema (ALTERs, views, an enum with `ADD VALUE`, a domain, CHECK
constraints, functions, `SET search_path`). `ci` is 8,000 files, 2,400 queries
and 400 tables. Queries cover joins and left/right/full outer joins, CTEs
(also recursive), `LATERAL`, `jsonb_build_object`/`jsonb_agg`, aggregates,
Literal-typed enum columns, views, functions, psycopg `execute` with `%s`,
`%(name)s` and `= ANY(%s)`, `TypedDict`, dataclass and tuple row types, `SQL[T]`
wrappers, bare `--sql` strings (for `rewrite`) and marked helpers
(`fetch_all`, `execute`).

## Metrics

Each is run in a fresh process; wall time is the median and peak RSS the
maximum of `--repeats` runs (RSS from `wait4` of that child).

- `check`: `sqlty check` over the project.
- `rewrite_check`: `sqlty rewrite --check` (no files change).
- `schema_load`: parse the 400-500 table schema through one `analyze_select`.
- `schema_load_unresolved_alters`: 2000 tables plus 2000 `ALTER TABLE` on tables
  that do not exist.
- `micro_us`: microseconds per `analyze_select` / `analyze_params` call for each
  query kind, schema cached.

```bash
make bench-project                                              # full size, JSON on stdout
uv run python benchmarks/bench_project.py --size full --keep /tmp/big  # keep the project
```

## CI budget

The `bench` job in `.github/workflows/tests.yml` builds the release extension
and runs `make bench-ci` (about 20 s). It fails when a metric is over its budget
in `baseline.json`:

- **Times** are divided by a calibration loop (a fixed pure-Python loop, best of
  5) timed in the same run, so a faster or slower runner moves baseline and
  result together. Tolerance is +50% (`--time-tolerance`). This beats comparing
  against `main` in the same job: no second build, and the baseline is a
  reviewable file. Its weakness is that a Python loop does not scale exactly
  like Rust code, hence the generous tolerance.
- **Peak RSS** is compared as is, +25% (`--rss-tolerance`).
- Times under 5 ms are reported, not budgeted; the per-kind micro timings are
  budgeted as two sums (`analyze_select`, `analyze_params`).
- The baseline is keyed by platform and size (`Darwin-ci`, `Linux-ci`). With no
  entry for the runner's platform the check passes and says so.

## Updating the baseline

After an intended change in speed or memory, on the machine type you want to
budget (CI is Linux: download the `bench-result` artifact of a green run, or run
on Linux), do:

```bash
make bench-baseline   # rewrites this platform's entry in benchmarks/baseline.json
```

Commit `baseline.json` with the change that moved the numbers. To add the
`Linux-ci` entry, put the `bench-result` artifact's JSON under that key.
