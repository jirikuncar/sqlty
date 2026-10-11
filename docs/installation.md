# Installation

## Requirements

- Python 3.15+

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
Windows (x64 and arm64). Elsewhere, and from a git checkout, pip builds the
sdist, which needs a [Rust toolchain](https://rustup.rs/). See
[Engine](reference/engine.md).

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
