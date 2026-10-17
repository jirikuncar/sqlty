# Installation

## Requirements

- Python 3.14 or 3.15, see [Supported Python versions](#supported-python-versions)
- A [Rust toolchain](https://rustup.rs/) only when no wheel matches your
  platform, or when installing from a git checkout

## Install

### pip

```bash
pip install sqlty
```

### uv

```bash
uv add sqlty
```

### poetry

```bash
poetry add sqlty
```

### From source

```bash
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty
uv sync --all-groups
```

### Optional dependencies

- `sqlty[psycopg]` installs psycopg 3 for the
  [psycopg helpers](guide/psycopg.md).

### Rust extension

sqlty's engine is a compiled Rust extension. Wheels are published for Linux
(x86_64 and aarch64, glibc and musl), macOS (Intel and Apple silicon) and
Windows (x64 and arm64). Elsewhere pip builds from the sdist, and a git
checkout builds from source; both need a [Rust toolchain](https://rustup.rs/). See
[Engine](reference/engine.md).

## Supported Python versions

sqlty supports the two latest CPython releases: 3.14 and 3.15. CI tests both, as
well as the free-threaded 3.14t, and every release ships wheels for both.

| Python | Status |
|--------|--------|
| 3.15 | Supported |
| 3.14 | Supported |
| 3.13 and older | Not supported |

When a new CPython is released, sqlty adds it and drops the oldest supported one.
The reason is maintenance cost. sqlty's checks follow Python's typing features,
and its wheels are built per interpreter.

An older Python stays supported only while a maintainer takes ownership of it:
testing it, fixing what breaks on it, and answering its issues. To volunteer, open
an issue.

!!! warning "Dropping a Python version can be a major release"
    If keeping an older Python would cause compatibility problems or a burden on
    the maintainers, a new major version can drop it at any time, without waiting
    for the next CPython release. `requires-python` keeps installers on the last
    release that supports your interpreter.

## Verify

```bash
sqlty version
sqlty --help
```

## Next steps

- [Quickstart](quickstart.md)
- [Use cases](use-cases.md)
- Contributing: [DEVELOPMENT.md](https://github.com/jirikuncar/sqlty/blob/main/DEVELOPMENT.md)
  covers the development setup and tests.
