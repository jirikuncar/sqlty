"""Test that magic codec generates QueryRow prototypes that users can alias."""

import pytest

from sqlty._decode import decode


def test_generates_queryrow_prototype() -> None:
    """Test that codec generates QueryRow_xxx Protocol."""
    source = b"""# coding: sqlty?return-format=namedtuple
from collections import namedtuple
from sqlty import SQL

# User defines their own type
User = namedtuple('User', ['id', 'name'])

query: SQL[User] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
"""

    result, _ = decode(source)

    # The codec should generate a Protocol that matches the structure
    assert "class QueryRow_724392cb(Protocol):" in result
    assert "id: int" in result
    assert "name: str" in result
    # User's type should be compatible with the Protocol
    assert "User = namedtuple('User', ['id', 'name'])" in result
    # Transformation should use the Protocol
    assert "SQL[QueryRow_724392cb]" in result


def test_preserves_user_type_alias() -> None:
    """Test that codec preserves user's type alias."""
    source = b"""# coding: sqlty?return-format=namedtuple
from collections import namedtuple
from sqlty import SQL

QueryRow_724392cb = namedtuple('QueryRow_724392cb', ['id', 'name'])
type User = QueryRow_724392cb

query: SQL[User] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
"""

    result, _ = decode(source)

    # Should preserve both the QueryRow definition and the alias
    assert "QueryRow_724392cb = namedtuple('QueryRow_724392cb', ['id', 'name'])" in result
    assert "type User = QueryRow_724392cb" in result
    # Transformation uses QueryRow_xxx (not User, since that's just an alias)
    assert "SQL[QueryRow_724392cb]" in result


def test_generates_unique_prototypes_for_different_queries() -> None:
    """Test that different queries get different QueryRow Protocol types."""
    source = b"""# coding: sqlty?return-format=namedtuple
from sqlty import SQL

query1 = "--sql SELECT id::INTEGER, name::TEXT FROM users"
query2 = "--sql SELECT email::TEXT, age::INTEGER FROM users"
"""

    result, _ = decode(source)

    # Should generate two different QueryRow Protocol types
    assert "QueryRow_" in result
    # Count how many unique QueryRow Protocol definitions
    import re

    matches = re.findall(r"class QueryRow_\w+\(Protocol\):", result)
    assert len(matches) == 2, f"Expected 2 QueryRow Protocol definitions, found {len(matches)}"


def test_doesnt_duplicate_existing_queryrow() -> None:
    """Test that codec doesn't duplicate if QueryRow is already defined."""
    source = b"""# coding: sqlty?return-format=namedtuple
from collections import namedtuple
from sqlty import SQL

# User manually defines the QueryRow type
QueryRow_724392cb = namedtuple('QueryRow_724392cb', ['id', 'name'])

query: SQL[QueryRow_724392cb] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
"""

    result, _ = decode(source)

    # Should have exactly one QueryRow definition (not duplicated)
    count = result.count("QueryRow_724392cb = namedtuple('QueryRow_724392cb', ['id', 'name'])")
    assert count == 1, f"Expected 1 QueryRow definition, found {count}"


def test_typeddict_generates_queryrow_class() -> None:
    """Test that TypedDict format generates QueryRow classes."""
    source = b"""# coding: sqlty?return-format=typeddict
import datetime
from typing import TypedDict
from sqlty import SQL

type User = QueryRow_724392cb
query: SQL[User] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
"""

    result, _ = decode(source)

    # Should generate QueryRow Protocol class
    assert "class QueryRow_724392cb(Protocol):" in result
    assert "id: int" in result
    assert "name: str" in result


def test_pydantic_generates_queryrow_model() -> None:
    """Test that Pydantic format generates QueryRow Protocol."""
    source = b"""# coding: sqlty?return-format=pydantic
from pydantic import BaseModel
from sqlty import SQL

type User = QueryRow_724392cb
query: SQL[User] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
"""

    result, _ = decode(source)

    # Should generate QueryRow Protocol class
    assert "class QueryRow_724392cb(Protocol):" in result
    assert "id: int" in result
    assert "name: str" in result
