"""Tests for AST-based incremental and full class updates in .pyi generator."""

from pathlib import Path

from sqlty._generator import CallInfo, TypeStubGenerator


def test_incremental_adds_only_missing_overloads(tmp_path: Path) -> None:
    """Incremental mode should insert only new overloads inside the class body."""
    # Prepare existing stub file with one overload and alias
    stub = (
        "from typing import overload, Literal\n"
        "from sqlty import SQL, SQLRegistry\n\n\n"
        "class MyQueries(SQLRegistry):\n"
        '    """Doc"""\n'
        "    @overload\n"
        "    @classmethod\n"
        "    def sql(\n"
        "        cls,\n"
        '        query: Literal["SELECT 1"],\n'
        "    ) -> SQL[tuple[int]]: ...\n"
        "    \n"
        "    @overload\n"
        "    @classmethod\n"
        "    def sql[T](cls, query: T) -> T: ...\n\n\n"
        "# Type alias for the sql function\n"
        "my_sql = MyQueries.sql\n"
    )
    stub_path = tmp_path / "my_queries.pyi"
    stub_path.write_text(stub)

    # Build new calls: the existing one plus a new one
    calls: list[CallInfo] = [
        {
            "literal": "SELECT 1",
            "return_type": "SQL[tuple[int]]",
            "lineno": 1,
            "file": "",
            "registry_source": "my_queries",
        },
        {
            "literal": "SELECT 2",
            "return_type": "SQL[tuple[int]]",
            "lineno": 1,
            "file": "",
            "registry_source": "my_queries",
        },
    ]

    gen = TypeStubGenerator()
    merged = gen.merge_stub_incremental(
        output_path=stub_path,
        new_calls=calls,
        class_name="MyQueries",
        base_class="SQLRegistry",
        export_name="my_sql",
    )

    # Should keep imports and alias intact
    assert merged.startswith("from typing import overload, Literal")
    assert merged.strip().endswith("my_sql = MyQueries.sql")

    # Should contain both SELECT 1 and SELECT 2 overloads only once each
    assert merged.count('Literal["SELECT 1"]') == 1
    assert merged.count('Literal["SELECT 2"]') == 1

    # Inserting again with the same calls shouldn't duplicate
    stub_path.write_text(merged)
    merged2 = gen.merge_stub_incremental(
        output_path=stub_path,
        new_calls=calls,
        class_name="MyQueries",
        base_class="SQLRegistry",
        export_name="my_sql",
    )
    assert merged2.count('Literal["SELECT 1"]') == 1
    assert merged2.count('Literal["SELECT 2"]') == 1


def test_full_mode_replaces_only_class_block(tmp_path: Path) -> None:
    """Full mode should rewrite only the class body, preserving imports and alias."""
    original = (
        "# Header comment\n"
        "from typing import overload, Literal\n"
        "from sqlty import SQL, SQLRegistry\n\n\n"
        "class MyQueries(SQLRegistry):\n"
        "    @overload\n"
        "    @classmethod\n"
        "    def sql(\n"
        "        cls,\n"
        '        query: Literal["SELECT A"],\n'
        "    ) -> SQL[tuple[str]]: ...\n"
        "    \n"
        "    @overload\n"
        "    @classmethod\n"
        "    def sql[T](cls, query: T) -> T: ...\n\n\n"
        "# Footer comment\n"
        "my_sql = MyQueries.sql\n"
    )
    stub_path = tmp_path / "my_queries.pyi"
    stub_path.write_text(original)

    # New calls for full rewrite
    calls: list[CallInfo] = [
        {
            "literal": "SELECT B",
            "return_type": "SQL[tuple[int]]",
            "lineno": 1,
            "file": "",
            "registry_source": "my_queries",
        },
    ]

    gen = TypeStubGenerator()
    merged = gen.merge_stub_incremental(
        output_path=stub_path,
        new_calls=calls,
        class_name="MyQueries",
        base_class="SQLRegistry",
        full_mode=True,
        export_name="my_sql",
    )

    # Imports and footer/alias preserved
    assert merged.splitlines()[0] == "# Header comment"
    assert "from typing import overload, Literal" in merged
    assert merged.rstrip().endswith("my_sql = MyQueries.sql")

    # Old overload removed, new one present
    assert 'Literal["SELECT A"]' not in merged
    assert 'Literal["SELECT B"]' in merged


def test_incremental_appends_class_if_missing(tmp_path: Path) -> None:
    """If the class isn't found, incremental should append a class block at end."""
    start = "from typing import overload, Literal\nfrom sqlty import SQL, SQLRegistry\n\n"
    stub_path = tmp_path / "missing_class.pyi"
    stub_path.write_text(start)

    calls: list[CallInfo] = [
        {
            "literal": "SELECT X",
            "return_type": "SQL[tuple[int]]",
            "lineno": 1,
            "file": "",
            "registry_source": None,
        },
    ]
    gen = TypeStubGenerator()
    merged = gen.merge_stub_incremental(
        output_path=stub_path,
        new_calls=calls,
        class_name="MyQueries",
        base_class="SQLRegistry",
        export_name="my_sql",
    )

    assert merged.startswith(start)
    assert "class MyQueries(SQLRegistry):" in merged
    assert merged.rstrip().endswith("my_sql = MyQueries.sql")
