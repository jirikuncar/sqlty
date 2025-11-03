# Installation Guide

## Requirements

- Python 3.14 or higher
- pip or uv for package management

## Installation Methods

### Using pip (Standard)

```bash
pip install sqltype
```

### Using uv (Recommended for Development)

[uv](https://docs.astral.sh/uv/) is a fast Python package installer and resolver:

```bash
# Install uv if you haven't already
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install sqltype
uv pip install sqltype
```

### Using poetry

```bash
poetry add sqltype
```

### From Source

For development or to get the latest features:

```bash
# Clone the repository
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty

# Install in development mode
uv pip install -e .

# Or with pip
pip install -e .
```

## Optional Dependencies

### Development Dependencies

For contributing or running tests:

```bash
uv pip install -e ".[dev]"
```

This includes:
- pytest (testing framework)
- mypy (type checking)
- ruff (linting and formatting)

### Type Checking Tools

SQLType requires mypy for type checking:

```bash
pip install mypy
```

## Verifying Installation

Check that sqltype is installed correctly:

```bash
# Check the package is installed
python -c "import sqltype; print(sqlty.__version__)"

# Check the CLI tool is available
sqlty --help
```

## IDE Integration

### VS Code

Install the Python and Pylance extensions for the best experience:

1. Install the [Python extension](https://marketplace.visualstudio.com/items?itemName=ms-python.python)
2. Install the [Pylance extension](https://marketplace.visualstudio.com/items?itemName=ms-python.vscode-pylance)
3. Enable type checking in settings:

```json
{
  "python.analysis.typeCheckingMode": "basic"
}
```

### PyCharm

PyCharm has built-in support for type checking:

1. Enable type checking: Settings → Editor → Inspections → Python → Type Checker
2. Configure mypy: Settings → Tools → Python Integrated Tools → Type Checker → mypy

## Next Steps

- Read the [Quick Start Guide](index.md#quick-start)
- Explore [Basic Usage](index.md#basic-usage)
- Check out the [Examples](index.md#examples)
