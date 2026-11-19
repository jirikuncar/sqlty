# Benchmarks

Three scripts; all need the release extension:
`uv run maturin develop --release --manifest-path rust/crates/sqlty-ext/Cargo.toml`.

| Script | What it measures | Make target |
|--------|------------------|-------------|
| `bench_project.py` | A generated large project: CLI wall time and peak RSS, and the language server's latency (`lsp.py`) | `make bench-project`, `make bench-ci`, `make bench-baseline` |
| `corpus.py` | `sqlty check` on pinned real repositories: wall time, peak RSS, diagnostics per rule | `make bench-corpus` |
| `test_benchmarks.py` | Each query kind, the schema load, `rewrite_source`, `check_source`, and completion at 50, 540 and 5000 tables, through CodSpeed | `make bench-codspeed` |

## The synthetic project

`synth.py` writes a deterministic project (seeded) into a temp directory at run
time; nothing is committed. `param-types` is on. `full` is 10,000 Python files, 3,000 queries and a
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
- `precommit`: `sqlty check` of 5 modules with queries, as a pre-commit hook passes them.
- `rewrite`: `sqlty rewrite`, which writes, on a fresh copy of the project per run.
- `sparse`: `sqlty check` of the same files with queries in 2% of them.
- `sql_files`: `sqlty check` of 2000 aiosql query files (3 queries each) beside 200 modules.
- `lsp_open`: `sqlty server` from `initialize` to the first diagnostics of an open
  module of 300 queries; RSS of the server.
- `lsp_edit`, `lsp_edit_p95`: a full-text change of that module to its diagnostics,
  median and p95 of 60 edits.
- `lsp_hover`, `lsp_completion`: request to response on a table name, total of 40.
- `schema_load`: parse the 400-500 table schema through one `analyze_select`.
- `schema_load_unresolved_alters`: 2000 tables plus 2000 `ALTER TABLE` on tables
  that do not exist.
- `schema_load_5000`: the synthetic schema at 5000 tables, the size completion must
  survive. Its RSS bounds the catalog and its name index.
- `micro_us`: microseconds per `analyze_select` / `analyze_params` call for each
  query kind, schema cached.

```bash
make bench-project                                              # full size, JSON on stdout
uv run python benchmarks/bench_project.py --size full --keep /tmp/big  # keep the project
uv run python benchmarks/bench_project.py --size ci --exe path/to/sqlty  # a native binary
```

## CI budget

The `bench` job in `.github/workflows/tests.yml` builds the release extension
and runs `make bench-ci` (about 40 s). It fails when a metric is over its budget
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
- A run over budget is measured again, up to three runs in all: another job on
  a shared runner can slow one run, and only the last run fails the job.
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

## CodSpeed

The `codspeed` job in `.github/workflows/tests.yml` runs `test_benchmarks.py` under
[CodSpeed](https://codspeed.io) on every push to `main` and every pull request, and
comments the change per benchmark on the pull request.

The upload needs the repository enabled on codspeed.io, and the job only measures
once the repository variable `CODSPEED` is `true`. Until then it runs each benchmark
once, unmeasured, so a broken benchmark still fails the pull request.

- It runs in simulation mode, which counts instructions instead of timing them. The
  engine is CPU-bound, so the count follows its speed, and it does not move with the
  load of a shared runner the way wall time does.
- The benchmarks reuse `synth.py`: the query kinds of the CI budget against its
  400-table schema, loaded once with `core.load_schema`, plus that schema's load and
  a module of every kind through `rewrite_source` and `check_source`.
- Locally, `make bench-codspeed` times them as plain pytest. Do not compare rows by
  the `Time (best)` column: pytest-codspeed 5.0.3 divides it by the iteration count
  twice. Use `make bench-rust` to compare a branch locally.

```bash
make bench-codspeed
uv run pytest benchmarks/ --codspeed -p no:randomly --no-cov -k check_source  # one benchmark
```

## Real repositories

`corpus.py` clones the repositories it pins (shallow, at one commit), writes a
`sqlty.toml` for each and runs `sqlty check`:

- `django`: no queries, so it measures what sqlty costs on a large codebase that
  does not use it.
- `goalert`: sqlc query files over its migration-built schema.
- `liberapay`: raw SQL through postgres.py's `db.one`/`db.all`, called on an
  attribute sqlty cannot type, so no query is found yet.

The `Real repositories` workflow runs it weekly and on pull requests that change it,
and shows the table in the job summary. Diagnostic counts that differ from
`corpus.json` are listed, not failed: a fix moves them as a regression does. A crash
or an `internal-error` fails. After an intended change, or when bumping a commit:

```bash
make bench-corpus ARGS=--update   # rewrites corpus.json
```
