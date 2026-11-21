# Python types from ty

## Goal

sqlty asks ty (`ty_python_semantic`) what a Python expression is instead of reading it with
its own resolver: the types of query arguments, the class of a driver or helper receiver, and
the modules and constants a file imports. Users get ty's inference where sqlty today sees only
declarations: locals bound to calls, narrowing (`is not None`, `isinstance`), return types,
attributes, unions. sqlty keeps everything SQL, the query strings it evaluates (`variants`,
`compose`, templates) and, until ty exposes class fields, row types.

Measured 2026-11-21 (ty 0.0.23, probe and numbers in the project's `ty-probe/symbols.md`): ty builds a semantic index
of the whole file before it answers anything; asking per definition instead of per expression
removes the dependence on function length (3.1 ms to 0.36 ms per query on 168-line modules).

## What moves to ty

| Today | Lines | With ty |
|---|--:|---|
| `scopes.rs` argument types (`arg_type`, `type_of`, `class_of`), `types/symbols.rs` aliases and `NewType` for parameters | 583 + part of 527 | `HasType` on the argument; `Parameter`/`Alias` definitions where the argument is a declared name |
| `helpers.rs` `receiver_class`, receiver detection in `drivers.rs` | 107 + part of 671 | type of the receiver, class by `Type::definition()` qualified name; `definitions_for_attribute` for helper methods |
| `index.rs` imported types/constants, `project/modules.rs`, `project/route.rs` | parts of 833 + 499 | ty's module resolver (`src` roots, stub paths, site-packages) |
| `proto/typed-receivers-a` (unmerged) | +2050 | dropped |

Stays: `types/extract.rs`, `resolve.rs`, `ty.rs`, `generic.rs`, `pydantic.rs` (row types:
TypedDict/NamedTuple/dataclass/BaseModel fields and their order). ty 0.0.23 keeps
`ClassLiteral`, `ClassType`, `TypedDictType` and MRO `pub(crate)`; see Open questions.

Mapping a ty `Type` to the parameter kinds sqlty checks uses public API only:
`is_assignable_to` against `KnownClass::{Int, Str, Bytes, Float, Bool, ...}.to_instance`,
`Type::none`, `NewType::concrete_base_type`, `UnionType` elements, `Type::display` for messages.

## Examples

New file `tests/mdtest/check/inferred_arguments.md`. Setup as in
`tests/mdtest/nullability/check.md` "A `NewType` argument" (`db.fetch_all` registered,
`items (id integer PRIMARY KEY, owner_id integer, ref uuid)`, `ids.py` with `OwnerId`).

### A local bound to a call

```python
from typing import NamedTuple

from db import fetch_all
from ids import OwnerId

class Row(NamedTuple):
    id: int
    owner_id: int

def lookup() -> OwnerId | None: ...

def insert() -> None:
    owner = lookup()
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=owner)
```

### Narrowed by `is not None`

```python
def insert_known() -> None:
    owner = lookup()
    if owner is not None:
        # error:
        fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=owner)
```

### A receiver typed by a return annotation

Setup of `tests/mdtest/check/driver_params.md`.

```python
import psycopg

def connect() -> psycopg.Connection: ...

conn = connect()
# error: [param-type-mismatch] "parameter 1 expects int, got bytes"
conn.execute("SELECT id FROM users WHERE id = %s", (b"1",))
```

### A receiver held by an attribute

```python
import psycopg

class Repo:
    def __init__(self, conn: psycopg.Connection) -> None:
        self.conn = conn

    def name(self) -> None:
        # error: [param-type-mismatch] "parameter 'id' expects int, got str"
        self.conn.execute("SELECT name FROM users WHERE id = %(id)s", {"id": "1"})
```

### What stays unknown

```python
def load(conn, ident):
    # error:
    conn.execute("SELECT name FROM users WHERE id = %(id)s", {"id": ident})
```

### Must not change

- Every existing mdtest, assertion text included: messages keep sqlty's spelling of the
  parameter kind (`int`, `str | None`), not ty's display, except `got X` where sqlty printed
  nothing before.
- `rewrite`, `strip`, `format`, `--fix` output; `No-exec harness intact` (ty reads, never runs).
- Result independent of the environment for the same files and settings (see Hosts).

## Hosts

One `sqlty-ty` crate owns the salsa database (own `Db`, as in `proto/typed-receivers-c`); the
engine (`sqlty-python`) asks it through a trait, no ty types or I/O in the engine.

| Host | File system | site-packages | Notes |
|---|---|---|---|
| CLI `check`, `breaking` | `OsSystem` | active venv, as `ty` finds it | one db per run, files parsed once (ty's parse reused by sqlty) |
| `sqlty server` | `OsSystem` + overlay of open buffers | same | long-lived db, `didChange` sets file text, salsa reuses the rest |
| Python extension (`sqlty.core`, mdtests) | `MemoryFileSystem` of the given sources | none | mdtest helper modules become files |
| wasm playground, VS Code web | `MemoryFileSystem` | none | vendored typeshed in the bundle |

Agreement: every host bundles the same driver stubs (psycopg, aiosqlite, asyncpg; sqlite3
is in typeshed) and resolves driver modules from them first, so a check does not depend on
which driver version is installed. Python version from `requires-python`, as today.

## Budget

Project rule: every check faster than a DB round trip (100 us per query), linear in input.

| ty 0.0.23, per query | short modules (18 lines) | 168-line modules |
|---|--:|--:|
| parse + semantic index (floor) | 14-20 us | 300-330 us |
| per-definition (`Parameter`/`Alias`) | 52-67 us | 343-357 us |
| per-expression (`HasType` on the argument) | 68-94 us | 3,129 us |
| sqlty main, whole `check` | 41 us | |

Linear in files, not in queries: a file with one query pays its whole index. RSS 47-350 KB per
file kept by salsa; LRU eviction per file after its checks run. Binary +34 MB (CLI), wasm
engine about doubles (prototype C). Measured with `make bench-ab` on `bench-ci` and the corpus
(django, goalert, liberapay), CPU and RSS against main; per-file and per-query numbers reported.
The 100 us rule does not hold on 168-line files (Open question 1).

## Phases

Each its own branch; a PR only on request.

1. Bump the `ruff_*` pins 0.0.21 to the release ty uses (0.0.23), alone.
2. `sqlty-ty` crate: `Db`, systems per host, bundled stubs, settings (`src`, `stub-paths`,
   site-packages), `catch_unwind` + time budget per file mapped to `internal-error`.
3. Argument types through ty; delete `scopes.rs` argument code and the parameter part of `symbols.rs`.
4. Receivers through ty; delete `receiver_class` and the driver receiver heuristics.
5. Module resolution and imported constants through ty; delete `project/modules.rs`, `route.rs`.
6. Python extension, LSP overlay, wasm: same mdtests in every host.

## Out of scope

Row types (fields of user classes), `variants`/`compose` string evaluation, SQL analysis,
reporting ty's own diagnostics.

## Risks

- Unsupported API: Astral says none of ty's crates are for outside use or planned as a crate
  (astral-sh/ty#1572). Weekly releases; `ruff_*` pins move with ty; API broke between 0.0.21
  and 0.0.23 (`Type::display` signature).
- Budget: whole-file index is the floor; above 100 us per query on longer files.
- Size: CLI +49%, clean build +63%, wasm engine doubles; 96 more crates.
- Threat model: ty reads every module a checked scope imports, site-packages included; a ty
  panic or cycle must not take a check down (`catch_unwind`, budget); new row in THREAT_MODEL.md.
- Two parsers' worth of trees unless sqlty reuses ty's `parsed_module` for its own walks.

## Open questions

1. Budget rule: (a) per query stays 100 us, ty only where sqlty's own reading fails;
   (b) the rule becomes per file (e.g. 1 ms per 100 lines) for ty's index; (c) ty everywhere,
   rule suspended for these paths.
2. Row types: (a) keep sqlty's reader (recommended); (b) members via `SemanticModel::attribute_completions`
   (types but no field order or `Required`/`ReadOnly`); (c) ask Astral to make class fields public.
3. site-packages: (a) bundled driver stubs first, then the venv (recommended); (b) venv only;
   (c) bundled stubs only, environment-independent.
4. wasm/VS Code web: (a) ship ty in the wasm engine (doubles download); (b) playground keeps
   today's resolver and may disagree with the CLI.
5. ty off switch: (a) none, ty always; (b) `SQLTY_TY=0` for debugging only.
