"""Example: configurable import style.

With ``import-style = "from sqlty import SQL as Q"`` in pyproject.toml,
``sqlty rewrite`` emits ``Q[T](...)`` instead of ``SQL[T](...)``.

Before rewrite::

    q = "--sql\\nSELECT id::INTEGER, name::TEXT FROM users"

After ``sqlty rewrite .``::

    from sqlty import SQL as Q

    q = Q[tuple[int, str]]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")

Run ``sqlty strip .`` to reverse.
"""
