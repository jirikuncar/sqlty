# SQLTy Documentation

Welcome to the SQLTy documentation!

## Quick Links

- **[Motivation](motivation.md)** - Why use SQLTy and when it's the right choice
- **[Main Documentation](index.md)** - Complete guide to SQLTy
- **[Installation Guide](installation.md)** - Setup and installation instructions
- **[API Reference](api.md)** - CLI commands, types, and configuration
- **[Magic Codec](magic-codec.md)** - Automatic string transformation

## Documentation Structure

### Getting Started

1. **[Motivation](motivation.md)**
   - When to use SQLTy
   - Why SQLTy exists
   - Trade-offs and comparisons

2. **[Installation](installation.md)**
   - Requirements (Python 3.12+)
   - Installation methods (pip, uv, from source)
   - Optional dependencies

3. **[Quick Start](index.md#quick-start)**
   - Writing `--sql` queries
   - Rewrite, strip, and check commands

### User Guide

4. **[Basic Usage](index.md#basic-usage)**
   - Writing SQL queries with `--sql` prefix
   - Type annotations via SQL casts
   - Schema-based inference

5. **[Usage Patterns](patterns.md)**
   - Rewrite/Strip (recommended)
   - Magic codec
   - Schema-based inference

6. **[Advanced Usage](advanced-usage.md)**
   - Custom SQL dialects
   - Multiple schemas
   - Return formats
   - Lint rule configuration
   - Pre-commit integration

### Reference

7. **[API Reference](api.md)**
   - CLI commands (rewrite, strip, check)
   - SQL type
   - Configuration reference
   - Type mappings
   - Lint rules

### Architecture (Internal)

8. **[IR Architecture](ir-architecture.md)** - Stub generation IR (deferred)

## Development

See **[DEVELOPMENT.md](../DEVELOPMENT.md)** for development setup, testing, and contribution guidelines.

## External Resources

- **[GitHub Repository](https://github.com/jirikuncar/sqlty)** - Source code and issues
- **[sqlglot Documentation](https://sqlglot.com/)** - SQL parsing and analysis

## License

SQLTy is licensed under the MIT License. See the LICENSE file for details.
