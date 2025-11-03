from pathlib import Path

from sqlty._config import load_config, merge_config


def test_pyproject_toml_schema_loading(tmp_path: Path) -> None:
    # Create pyproject.toml with tool.sqlty config
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""
[tool.sqlty]
dialect = "sqlite"
schemas = { users_queries = "users.sql", orders_queries = "orders.sql" }
""")
    # Load config
    config = load_config(tmp_path)
    assert config["dialect"] == "sqlite"
    assert config["schemas"]["users_queries"] == "users.sql"
    assert config["schemas"]["orders_queries"] == "orders.sql"
    # CLI override
    cli_args = {
        "dialect": "postgres",
        "schemas": {"users_queries": "override.sql"},
    }
    merged = merge_config(config, cli_args)
    assert merged["dialect"] == "postgres"
    assert merged["schemas"]["users_queries"] == "override.sql"
    assert merged["schemas"]["orders_queries"] == "orders.sql"
