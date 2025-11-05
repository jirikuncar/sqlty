# Motivation

## When to Use SQLTy

**SQLTy is designed for teams working directly with SQL in large Python codebases.**

If you're using an ORM (SQLAlchemy, Django ORM, etc.), you should use its built-in type system instead. ORMs provide comprehensive typing support that's tightly integrated with their query builders and model definitions.

SQLTy targets a specific use case: projects that write raw SQL queries but need static type checking without runtime overhead.

## Why SQLTy Exists

### Large Codebases

In large applications with hundreds or thousands of SQL queries, maintaining type safety becomes critical. Manual type annotations are error-prone and drift from actual query structure. SQLTy automatically generates accurate type stubs by analyzing your SQL queries, ensuring types stay synchronized with your code.

### Fast Import Times

Traditional approaches to SQL typing often involve runtime introspection or dynamic type generation, which slows down module imports. This becomes painful in large codebases where import time directly impacts developer productivity and CI/CD performance.

SQLTy uses stub files (`.pyi`) for type information, which are read by type checkers like mypy but never imported at runtime. Your application imports remain fast regardless of how many queries you have.

### Minimal Runtime Overhead

Type checking happens during static analysis, not at runtime. SQLTy generates stub files that mypy uses for type checking, but your actual application code has no runtime dependencies on these stubs. The runtime overhead is minimal—just a simple passthrough method and an optional environment variable check during class definition.

## How It Works

1. **Write SQL queries** with explicit type casts (`::INTEGER`, `::TEXT`, etc.) or provide path to SQL schema dump
2. **Run `sqlty` command** to analyze your code and generate `.pyi` stub files
3. **Type check with mypy** - stubs provide precise types for query results
4. **Runtime execution** uses your queries as-is, no overhead from type checking

The type checking layer is completely separated from runtime execution.

## Trade-offs

### When SQLTy is a Good Fit

- You write raw SQL queries (not using an ORM)
- You have a large codebase with many queries
- You need static type checking for SQL results
- You want fast import times and minimal runtime overhead
- You can annotate your SQL with type casts or provide a schema dump

### When to Use Something Else

- **Using an ORM**: Use its native type system (e.g., SQLAlchemy 2.0+ with mypy plugin)
- **Small projects**: Manual type annotations may be simpler
- **Cannot modify SQL or provide schema**: SQLTy requires type casts in queries or a schema dump for inference
- **Need runtime validation**: SQLTy focuses on static analysis, not runtime checks

## Comparison with Other Approaches

### vs. ORM Type Systems

ORMs provide tight integration between models, queries, and types. Use them if you're building on an ORM. SQLTy is for teams that choose raw SQL for performance, flexibility, or legacy reasons.

### vs. Runtime Type Validation

Libraries like Pydantic focus on runtime validation. SQLTy focuses on static type checking at analysis time. You can use both together if needed.

### vs. Manual Type Annotations

Manual annotations work for small codebases but become unmaintainable at scale. SQLTy automates the process while keeping types accurate and synchronized with your queries.

## Real-World Benefits

- **Catch errors before runtime**: Type checker finds mismatches between expected and actual query results
- **Refactoring confidence**: Change query structure and mypy tells you everywhere that needs updating
- **IDE support**: Get autocomplete and inline documentation for query results
- **Minimal performance impact**: Type checking overhead is negligible at runtime
- **Fast development cycles**: Quick imports mean fast test runs and CI builds

SQLTy brings static typing to SQL without compromising on performance or development speed.
