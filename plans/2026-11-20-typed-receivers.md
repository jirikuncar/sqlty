# Typed receivers

A helper method is checked when sqlty knows the class of its receiver, the way a type
checker knows it. No name matching: `feat/function-receivers` (`receivers = ["db"]`) is
dropped.

## Evidence

ty 0.0.107 on the liberapay patterns, with postgres 4.0 installed (no annotations, no
`py.typed`, no typeshed stubs):

| Receiver | ty | ty + a `postgres` stub |
|----------|----|------------------------|
| `db = DB(url)` (`class DB(Postgres)`) | `DB` | `DB` |
| `self.db` in a `Model` subclass (`Model.db = None`, set at runtime) | `None \| Unknown` | `Postgres` |
| `with db.get_cursor() as cursor` | `Unknown` | `SimpleCursorBase` |
| `website.db` where `Website` annotates `db: DB` | `DB` | `DB` |
| unannotated parameter `db` | `Unknown` | `Unknown` |
| `d = setup()`, `setup` unannotated | `Unknown` | `Unknown` |
| `cls.db` in `Payday` (`db = None`, set from outside) | `None \| Unknown` | `None \| Unknown` |
| `self.db = DB(...)` in `__init__`, `shared = DB(...)` in the class body | `DB` | `DB` |
| `def make() -> DB`, `@contextmanager -> Iterator[DB]`, `cast(DB, x)`, `isinstance(x, DB)` | `DB` | `DB` |

liberapay at cf2592f has 458 `one/all/run/get_cursor` calls on `db`, `cursor`, `c` or
`website` receivers:

| Receiver | Calls | Typed after this plan |
|----------|-------|-----------------------|
| `self.db`/`cls.db` in `Model` subclasses (Participant 104, ExchangeRoute 12, ...) | 137 | with a `postgres` stub |
| `cursor` from `with self.db.get_cursor() as cursor` in those classes | 50 | with a `postgres` stub |
| other `with ... as cursor` | 47 | no |
| unannotated parameters | 88 | no |
| `website.db` (pando `Website`, attribute set at runtime) | 40 | no |
| `self.db`/`cls.db` in `Payday`, `Harness`, `MixinTeam`, `Website` (no declaration) | 45 | no |
| other (`db = website.db`, `cursor or self.db`, `NoDB()`, ...) | 51 | a few |

So liberapay as published gets ~0, and ~190 with a stub of the untyped library, which is
what ty and pyright users do too (`extra-paths`, `stubPath`). The rest is `Unknown` to ty
as well.

## What sqlty has

`helpers::receiver_class` (`sqlty-python`) types a receiver from: `self`/`cls`, a name
annotated or assigned `Cls(...)` in its scope or the module, `self.attr` annotated in the
class body or assigned `Cls(...)` in a method. Only the class itself is searched for the
method: inherited methods, attribute chains (`website.db`), return types, `with ... as`
and other modules' classes are not followed.

## Proposal

One resolver, `receivers.rs`, gives the class of an expression from declarations only.
Each step is something ty does; anything else is unknown and the call is skipped.

1. Names: parameter and variable annotations, `x = Cls(...)`, `x = <typed expr>`.
   Several different assignments in a scope make it unknown, as `note` does today.
2. Attributes: the class of the base, then the attribute declared on it or a base class:
   class-body annotation (`ClassVar` too), class-body `x = Cls(...)`, `self.x: T` or
   `self.x = Cls(...)` in a method. `cls.x` and `Cls.x` alike. Chains (`self.cron.website.db`).
3. Calls: a class is its instance; a function or method returns its annotation (`Self`
   is the receiver's class); `typing.cast(T, x)` is `T`. No return inference.
4. `with E as x`: `__enter__`'s (or `__aenter__`'s) annotation; a `@contextmanager`
   function annotated `Iterator[T]`/`Generator[T, ...]` gives `T`.
5. `T | None` and `Optional[T]` are `T`; a union of different classes is unknown.
   `isinstance(x, T)` narrowing in `if`/`assert`: second milestone.
6. Methods are found through the bases, in MRO order (C3 is not needed for single
   inheritance; with several bases, the first one defining the method wins, as in ty
   for non-diamond hierarchies).
7. Stubs: a `.pyi` wins over the `.py` of the same module, found on `stub-paths`
   (default `["typings"]`, pyright's default) before the current module roots. A stub
   method whose query parameter is `Annotated[LiteralString, SQL]` is a helper with no
   `[[tool.sqlty.functions]]` entry.

Classes are read from modules the helper index already loads (checked paths, `src/`, the
project, `sys.path`); never imported or executed. Class facts are cached per module like
row types (`Tree::definitions`).

## Config

```toml
[tool.sqlty]
stub-paths = ["typings"]

[[tool.sqlty.functions]]           # unchanged; stays for libraries without a stub
import-path = "postgres.Postgres.one"
query-param = "sql"
```

`receivers` goes. A registered method also applies to subclasses (`DB(Postgres)`).

## Not chosen: embedding ty

`ty_python_semantic` 0.0.23 is on crates.io but describes itself as "an internal
component crate of Ruff": no stable API, a salsa database and vendored typeshed to set
up, ruff crate versions pinned in lockstep with ours (`ruff_python_ast =0.0.21`), and a
full type check of the project on every run. Exact ty semantics, at a cost in binary size,
check time and upgrades. Revisit if ty ships a library API.

## Milestones

1. Resolver for steps 1-3 and 5-6 in one module, replacing `receiver_class`; mdtests in
   `tests/mdtest/check/receivers.md` (each row of the evidence table), trycmd for a
   cross-module class.
2. `with ... as`, `@contextmanager`, `isinstance`.
3. `stub-paths` and `.pyi` precedence; helpers marked in stubs; docs in
   `docs/guide/helpers.md`.
4. Corpus: a `postgres` stub in `benchmarks/corpus/stubs/` for liberapay; report the
   count; bench-ci within noise (the resolver runs only for calls whose method name is a
   registered helper's).
