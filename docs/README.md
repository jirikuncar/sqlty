# SQLTy Documentation

Welcome to the SQLTy documentation!

## Quick Links

- **[Motivation](motivation.md)** - Why use SQLTy and when it's the right choice
- **[Main Documentation](index.md)** - Complete guide to SQLTy
- **[Installation Guide](installation.md)** - Setup and installation instructions
- **[API Reference](api.md)** - Detailed API documentation
- **[Magic Codec](magic-codec.md)** - Automatic string transformation

## Documentation Structure

### Getting Started

1. **[Motivation](motivation.md)**
   - When to use SQLTy
   - Why SQLTy exists
   - Trade-offs and comparisons

2. **[Installation](installation.md)**
   - Requirements (Python 3.12+)
   - Installation methods (pip, uv, poetry, from source)
   - Optional dependencies
   - IDE integration

3. **[Quick Start](index.md#quick-start)**
   - Basic setup
   - Writing queries
   - Rewrite and strip commands
   - Type checking

### User Guide

4. **[Basic Usage](index.md#basic-usage)**
   - Creating a SQL registry
   - Writing SQL queries
   - Type annotations in SQL
   - Generating stub files

5. **[Advanced Usage](advanced-usage.md)**
   - Multi-registry pattern
   - Custom SQL dialects
   - Schema-based inference
   - Rule configuration

### Reference

6. **[API Reference](api.md)**
   - `@sqlty` decorator
   - SQL type
   - Generator CLI
   - Type mappings
   - Lint rules

### Architecture

7. **[IR Architecture](ir-architecture.md)** - Internal representation details
8. **[asttokens Integration](asttokens-integration.md)** - Source transformation internals
9. **[Patterns](patterns.md)** - Usage patterns and best practices

## Development

See **[DEVELOPMENT.md](../DEVELOPMENT.md)** for development setup, testing, and contribution guidelines.

## External Resources

- **[GitHub Repository](https://github.com/jirikuncar/sqlty)** - Source code and issues
- **[mypy Documentation](https://mypy.readthedocs.io/)** - Python type checking
- **[sqlglot Documentation](https://sqlglot.com/)** - SQL parsing and analysis
- **[PEP 484](https://peps.python.org/pep-0484/)** - Type hints specification

## License

SQLTy is licensed under the MIT License. See the LICENSE file for details.
