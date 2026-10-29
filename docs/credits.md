# Credits

sqlty builds on other people's work: the libraries it ships with, the tools that
build and test it, the specifications it follows, and the projects it was
compared with. This page lists them with their licenses. Projects that solve
the same problem in other ways are in
[Prior art](motivation.md#prior-art), and the articles that shaped the design
are in [Further reading](motivation.md#further-reading).

## People

| Who | Role |
|-----|------|
| [Jiri Kuncar](https://github.com/jirikuncar) | Author and maintainer |
| [Marcelo Trylesinski](https://github.com/Kludex) | Contributor |

Some changes were written with AI coding agents (Claude, GitHub Copilot). The
contributor who opened each pull request reviewed it, and each commit names the
agent in a `Co-authored-by` trailer. See
[AI-assisted contributions](https://github.com/jirikuncar/sqlty/blob/main/CONTRIBUTING.md#ai-assisted-contributions).
Dependabot keeps the dependencies current.

## What sqlty is built from

The wheel bundles a compiled Rust extension. The crates it is built from, and
the Python packages sqlty depends on at runtime:

| Project | Used for | License |
|---------|----------|---------|
| [sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs) | Parsing SQL in every dialect | Apache-2.0 |
| [PyO3](https://pyo3.rs) | The Python bindings of the Rust engine | MIT or Apache-2.0 |
| [regex](https://github.com/rust-lang/regex) | Patterns in the source scanner and the SQL preprocessor | MIT or Apache-2.0 |
| [rustc-hash](https://github.com/rust-lang/rustc-hash) | Hash maps of the schema catalog | MIT or Apache-2.0 |
| [stacker](https://github.com/rust-lang/stacker), [recursive](https://github.com/orlp/recursive) | Stack for deeply nested expressions | MIT or Apache-2.0, MIT |
| [thiserror](https://github.com/dtolnay/thiserror) | Error types | MIT or Apache-2.0 |
| [lsp-server](https://github.com/rust-lang/rust-analyzer/tree/master/lib/lsp-server), [lsp-types](https://github.com/gluon-lang/lsp-types), [crossbeam-channel](https://github.com/crossbeam-rs/crossbeam) | The language server of `sqlty server` | MIT or Apache-2.0, MIT, MIT or Apache-2.0 |
| [serde](https://serde.rs), [toml](https://github.com/toml-rs/toml) | Reading the inference rules at build time; serde and [`serde_json`](https://github.com/serde-rs/json) also the messages of `sqlty server` | MIT or Apache-2.0 |
| [pathspec](https://github.com/cpburnz/python-pathspec) | `.gitignore` and exclude patterns | MPL-2.0 |
| [typing_extensions](https://github.com/python/typing_extensions) | Typing features before Python 3.15 | PSF-2.0 |
| [psycopg](https://www.psycopg.org/psycopg3/) | The optional `sqlty[psycopg]` helpers | LGPL-3.0 |

## Tools

| Project | Used for | License |
|---------|----------|---------|
| [maturin](https://www.maturin.rs) | Building the wheels | MIT or Apache-2.0 |
| [uv](https://docs.astral.sh/uv/) | Environments and dependency locking | MIT or Apache-2.0 |
| [Ruff](https://github.com/astral-sh/ruff) | Linting and formatting | MIT |
| [mypy](https://github.com/python/mypy), [ty](https://github.com/astral-sh/ty), [Pyright](https://github.com/microsoft/pyright) | Type checking sqlty, and checking `SQL[T]` with each | MIT |
| [prek](https://prek.j178.dev) | Git hooks | MIT |
| [pytest](https://github.com/pytest-dev/pytest), [pytest-cov](https://github.com/pytest-dev/pytest-cov), [pytest-randomly](https://github.com/pytest-dev/pytest-randomly) | The test suite | MIT |
| [Hypothesis](https://hypothesis.readthedocs.io) | Fuzzing the analyzer and the Lean model | MPL-2.0 |
| [inline-snapshot](https://github.com/15r10nk/inline-snapshot) | Python snapshot tests | MIT |
| [insta](https://insta.rs) | Rust snapshot tests | Apache-2.0 |
| [Criterion.rs](https://github.com/bheisler/criterion.rs) | Engine benchmarks | MIT or Apache-2.0 |
| [pytest-codspeed](https://github.com/CodSpeedHQ/pytest-codspeed) | Benchmarks CodSpeed measures on every commit | MIT |
| [DuckDB](https://duckdb.org) | Checking DuckDB's value types | MIT |
| [Pydantic](https://docs.pydantic.dev) | Checking the table of Pydantic types against Pydantic | MIT |
| [PyYAML](https://github.com/yaml/pyyaml) | Reading hook and workflow files in tests | MIT |
| [Lean 4](https://lean-lang.org), [elan](https://github.com/leanprover/elan) | Proofs about the type mapping | Apache-2.0; elan MIT or Apache-2.0 |
| [Zensical](https://zensical.org) | This documentation site | MIT |

## Ideas and specifications

- The Markdown test format follows
  [ty's mdtest](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md)
  (MIT).
- PEPs:
  - [249](https://peps.python.org/pep-0249/): the DB-API, whose paramstyles sqlty reads.
  - [675](https://peps.python.org/pep-0675/): `LiteralString`.
  - [661](https://peps.python.org/pep-0661/): sentinel values, which `UNTYPED` is.
  - [718](https://peps.python.org/pep-0718/): subscriptable functions, an alternative to `SQL[T](...)`.
  - [728](https://peps.python.org/pep-0728/): closed and `extra_items` `TypedDict`s, which row type checks follow.
  - [747](https://peps.python.org/pep-0747/): `TypeForm`, for the psycopg helpers' row types.
  - [750](https://peps.python.org/pep-0750/): template strings, which sqlty reads as queries.
- `:name` placeholders are read with the rule of
  [SQLAlchemy](https://www.sqlalchemy.org)'s `text()` (MIT).
- The code of conduct is the
  [Contributor Covenant](https://www.contributor-covenant.org/version/3/0/)
  (CC BY-SA 4.0).

## Reference documentation

Every inference rule cites the documentation it follows, fetched and checked:

- [PostgreSQL 17](https://www.postgresql.org/docs/17/)
- [MySQL 8.4 Reference Manual](https://dev.mysql.com/doc/refman/8.4/en/)
- [SQLite](https://www.sqlite.org/docs.html), in particular
  [Datatypes](https://www.sqlite.org/datatype3.html) and
  [JSON functions](https://www.sqlite.org/json1.html)
- [DuckDB](https://duckdb.org/docs/current/clients/python/conversion.html)
  Python conversion rules
- The [Python standard library](https://docs.python.org/3/library/)

## Compared with

The [type mapping](reference/types.md) was checked against what these projects
return or declare. None of them is bundled: the comparisons in
[`scripts/compat/`](https://github.com/jirikuncar/sqlty/tree/main/scripts/compat)
install each one in a separate environment.

| Project | Compared | License |
|---------|----------|---------|
| [psycopg](https://www.psycopg.org/psycopg3/), [psycopg2](https://github.com/psycopg/psycopg2) | PostgreSQL values, the reference driver | LGPL-3.0, LGPL with exceptions |
| [asyncpg](https://github.com/MagicStack/asyncpg) | PostgreSQL values | Apache-2.0 |
| [PyMySQL](https://github.com/PyMySQL/PyMySQL) | MySQL values, the reference driver | MIT |
| [mysqlclient](https://github.com/PyMySQL/mysqlclient) | MySQL values | GPL-2.0-or-later |
| [sqlite3](https://docs.python.org/3/library/sqlite3.html), [aiosqlite](https://github.com/omnilib/aiosqlite) | SQLite values | PSF-2.0, MIT |
| [sqlglot](https://github.com/tobymao/sqlglot) | Its type annotation test fixtures | MIT |
| [SQLAlchemy](https://www.sqlalchemy.org) | Column and expression types | MIT |
| [Django](https://www.djangoproject.com) | Field and expression types | BSD-3-Clause |
| [Tortoise ORM](https://github.com/tortoise/tortoise-orm) | Field types | Apache-2.0 |
| [SQLModel](https://github.com/fastapi/sqlmodel) | Field types | MIT |
| [Piccolo](https://github.com/piccolo-orm/piccolo) | Column types | MIT |
| [ormar](https://github.com/collerek/ormar) | Field types | MIT |
| [peewee](https://github.com/coleifer/peewee) | Field types | MIT |
| [Pony ORM](https://github.com/ponyorm/pony) | Attribute types | Apache-2.0 |
| [SQLObject](https://github.com/sqlobject/sqlobject) | Column types | LGPL |
| [Synapse](https://github.com/element-hq/synapse) | Running sqlty on a large project's schema and queries | AGPL-3.0 |

## Formal verification

The [Lean model](https://github.com/jirikuncar/sqlty/tree/main/proofs) was chosen after reviewing
these approaches:

- [Aeneas](https://github.com/AeneasVerif/aeneas) and
  [hax](https://github.com/cryspen/hax): extracting Rust to a proof assistant.
- [Verus](https://github.com/verus-lang/verus),
  [Creusot](https://github.com/creusot-rs/creusot) and
  [Prusti](https://github.com/viperproject/prusti-dev): proofs written as
  annotations in Rust.
- [Surveying the Rust Verification Landscape](https://arxiv.org/abs/2410.01981).
