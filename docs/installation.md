# Installation Guide

## Requirements

- Python 3.12 or higher
- pip or uv for package management

## Installation Methods

### Using pip (Standard)

```bash
pip install sqlty
```

### Using uv (Recommended for Development)

[uv](https://docs.astral.sh/uv/) is a fast Python package installer and resolver:

```bash
# Install uv if you haven't already
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install sqlty
uv pip install sqlty
```

### Using poetry

```bash
poetry add sqlty
```

### From Source

For development or to get the latest features:

```bash
# Clone the repository
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty

# Install in development mode with uv
uv sync --all-groups

# Or with pip
pip install -e .
```

## Optional Dependencies

### Development Dependencies

For contributing or running tests:

```bash
uv sync --all-groups
```

This includes:
- pytest (testing framework)
- mypy (type checking)
- ruff (linting and formatting)

### Type Checking Tools

SQLTy works with mypy for type checking:

```bash
pip install mypy
```

## Verifying Installation

Check that sqlty is installed correctly:

```bash
# Check the CLI tool is available
sqlty version

# See available commands
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

1. Enable type checking: Settings -> Editor -> Inspections -> Python -> Type Checker
2. Configure mypy: Settings -> Tools -> Python Integrated Tools -> Type Checker -> mypy

## Next Steps

- Read the [Quick Start Guide](index.md#quick-start)
- Explore [Basic Usage](index.md#basic-usage)
- Check out the [Examples](../examples/)
