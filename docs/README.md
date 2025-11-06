# SQLType Documentation

Welcome to the SQLType documentation!

## Quick Links

- **[Motivation](motivation.md)** - Why use SQLTy and when it's the right choice
- **[Main Documentation](index.md)** - Complete guide to SQLType
- **[Installation Guide](installation.md)** - Setup and installation instructions
- **[API Reference](api.md)** - Detailed API documentation

## Documentation Structure

### Getting Started

1. **[Motivation](motivation.md)**
   - When to use SQLTy
   - Why SQLTy exists
   - Trade-offs and comparisons
   - Real-world benefits

2. **[Installation](installation.md)**
   - Requirements
   - Installation methods (pip, uv, poetry, from source)
   - Optional dependencies
   - IDE integration
   - Verifying installation

3. **[Quick Start](index.md#quick-start)**
   - Basic setup
   - Writing queries
   - Generating stubs
   - Type checking

### User Guide

4. **[Basic Usage](index.md#basic-usage)**
   - Creating a SQL registry
   - Writing SQL queries
   - Type annotations in SQL
   - Generating stub files
   - Using the generated stubs

5. **[Advanced Usage](index.md#advanced-usage)**
   - Multi-registry pattern
   - Custom SQL dialects
   - Incremental mode
   - Custom class and export names
   - Specifying output paths

### Reference

6. **[API Reference](api.md)**
   - `@sqlty` decorator
   - SQL type
   - Generator CLI
   - Type mappings
   - Generated stub structure
   - Type checking integration
   - Error handling
   - Performance considerations

### Understanding SQLType

7. **[Internals](index.md#internals)**
   - Architecture overview
   - Type inference engine
   - AST analysis
   - Stub generation process

### Examples

8. **[Examples](index.md#examples)**
   - Simple query registry
   - Multi-registry application
   - Real-world usage patterns

## Contributing

See the [Contributing section](index.md#contributing) in the main documentation.

## External Resources

- **[GitHub Repository](https://github.com/jirikuncar/sqlty)** - Source code and issues
- **[mypy Documentation](https://mypy.readthedocs.io/)** - Python type checking
- **[sqlglot Documentation](https://sqlglot.com/)** - SQL parsing and analysis
- **[PEP 484](https://peps.python.org/pep-0484/)** - Type hints specification

## Support

For questions, issues, or contributions:

- Open an issue on [GitHub](https://github.com/jirikuncar/sqlty/issues)
- Check existing [documentation](index.md)
- Review the [examples](index.md#examples)

## License

SQLType is licensed under the MIT License. See the LICENSE file for details.
