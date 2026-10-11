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
- [isort](https://pycqa.github.io/isort/), if installed, places the imports the
  [magic codec](guide/magic-codec.md) adds.

### Rust extension

Wheels include the Rust extension that speeds up type inference. Building from
source, for example from a git checkout, needs a
[Rust toolchain](https://rustup.rs/) because maturin compiles the extension.
If the extension cannot be imported, sqlty falls back to pure Python. See
[Backends](reference/backends.md).

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
