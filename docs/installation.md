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

## Verify

```bash
sqlty version
sqlty --help
```

## Development Dependencies

For contributing or running tests:

```bash
uv sync --all-groups
```

Includes: pytest, mypy, ruff.

## Next Steps

- [Quick Start](index.md#quick-start)
- [Examples](examples.md)
