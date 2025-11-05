"""Tests for UPDATE/DELETE/INSERT/MERGE with RETURNING clause support."""

import pytest

from sqlty._sql_analyzer import SQLAnalyzer


@pytest.fixture
def analyzer():
    """Create SQL analyzer instance with postgres dialect."""
    return SQLAnalyzer(dialect="postgres")


# UPDATE with RETURNING tests


def test_update_returning_single_column(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with RETURNING single column."""
    columns = analyzer.analyze_select(
        "UPDATE users SET name = 'John' WHERE id = 1 RETURNING id::INTEGER"
    )

    assert columns is not None
    assert len(columns) == 1
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[0].sql_type == "INT"


def test_update_returning_multiple_columns(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with RETURNING multiple columns."""
    columns = analyzer.analyze_select(
        "UPDATE users SET name = 'John' WHERE id = 1 RETURNING id::INTEGER, name::TEXT"
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[1].name == "name"
    assert columns[1].python_type == "str"


def test_update_returning_mixed_types(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with RETURNING various types."""
    columns = analyzer.analyze_select(
        """
        UPDATE users
        SET name = 'John', age = 30, active = true
        WHERE id = 1
        RETURNING id::INTEGER, name::TEXT, age::INTEGER, active::BOOLEAN, updated_at::TIMESTAMP
        """
    )

    assert columns is not None
    assert len(columns) == 5
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"
    assert columns[2].python_type == "int"
    assert columns[3].python_type == "bool"
    assert columns[4].python_type == "datetime.datetime"


def test_update_returning_all_columns(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with RETURNING *."""
    columns = analyzer.analyze_select("UPDATE users SET name = 'John' RETURNING *")

    assert columns is not None
    # RETURNING * is treated as a single projection expression by sqlglot.
    # Without schema information, it defaults to a single column with unknown type (str).
    # In practice, this would return all columns from the table, but type inference
    # requires explicit casts or schema information.
    assert len(columns) == 1


def test_update_returning_generate_type(analyzer: SQLAnalyzer) -> None:
    """Test generate_return_type for UPDATE with RETURNING."""
    return_type = analyzer.generate_return_type(
        "UPDATE users SET name = 'John' RETURNING id::INTEGER, name::TEXT"
    )

    assert return_type == "tuple[int, str]"


# DELETE with RETURNING tests


def test_delete_returning_single_column(analyzer: SQLAnalyzer) -> None:
    """Test DELETE with RETURNING single column."""
    columns = analyzer.analyze_select("DELETE FROM users WHERE id = 1 RETURNING id::INTEGER")

    assert columns is not None
    assert len(columns) == 1
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[0].sql_type == "INT"


def test_delete_returning_multiple_columns(analyzer: SQLAnalyzer) -> None:
    """Test DELETE with RETURNING multiple columns."""
    columns = analyzer.analyze_select(
        "DELETE FROM users WHERE active = false RETURNING id::INTEGER, name::TEXT, email::TEXT"
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[1].name == "name"
    assert columns[1].python_type == "str"
    assert columns[2].name == "email"
    assert columns[2].python_type == "str"


def test_delete_returning_mixed_types(analyzer: SQLAnalyzer) -> None:
    """Test DELETE with RETURNING various types."""
    columns = analyzer.analyze_select(
        """
        DELETE FROM orders
        WHERE status = 'cancelled'
        RETURNING order_id::INTEGER, total::FLOAT, created_at::TIMESTAMP, cancelled::BOOLEAN
        """
    )

    assert columns is not None
    assert len(columns) == 4
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "float"
    assert columns[2].python_type == "datetime.datetime"
    assert columns[3].python_type == "bool"


def test_delete_returning_generate_type(analyzer: SQLAnalyzer) -> None:
    """Test generate_return_type for DELETE with RETURNING."""
    return_type = analyzer.generate_return_type(
        "DELETE FROM users WHERE id = 1 RETURNING id::INTEGER"
    )

    assert return_type == "tuple[int]"


# INSERT with RETURNING tests


def test_insert_returning_single_column(analyzer: SQLAnalyzer) -> None:
    """Test INSERT with RETURNING single column."""
    columns = analyzer.analyze_select(
        "INSERT INTO users (name) VALUES ('John') RETURNING id::INTEGER"
    )

    assert columns is not None
    assert len(columns) == 1
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[0].sql_type == "INT"


def test_insert_returning_multiple_columns(analyzer: SQLAnalyzer) -> None:
    """Test INSERT with RETURNING multiple columns."""
    columns = analyzer.analyze_select(
        """
        INSERT INTO users (name, email)
        VALUES ('John', 'john@example.com')
        RETURNING id::INTEGER, name::TEXT, created_at::TIMESTAMP
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[1].name == "name"
    assert columns[1].python_type == "str"
    assert columns[2].name == "created_at"
    assert columns[2].python_type == "datetime.datetime"


def test_insert_returning_with_multiple_values(analyzer: SQLAnalyzer) -> None:
    """Test INSERT with multiple VALUES and RETURNING."""
    columns = analyzer.analyze_select(
        """
        INSERT INTO users (name, email)
        VALUES
            ('John', 'john@example.com'),
            ('Jane', 'jane@example.com')
        RETURNING id::INTEGER, name::TEXT
        """
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"


def test_insert_returning_generate_type(analyzer: SQLAnalyzer) -> None:
    """Test generate_return_type for INSERT with RETURNING."""
    return_type = analyzer.generate_return_type(
        "INSERT INTO users (name) VALUES ('John') RETURNING id::INTEGER, name::TEXT"
    )

    assert return_type == "tuple[int, str]"


# MERGE with RETURNING tests


def test_merge_returning_single_column(analyzer: SQLAnalyzer) -> None:
    """Test MERGE with RETURNING single column."""
    columns = analyzer.analyze_select(
        """
        MERGE INTO users
        USING updates ON users.id = updates.id
        WHEN MATCHED THEN UPDATE SET name = updates.name
        WHEN NOT MATCHED THEN INSERT (name) VALUES (updates.name)
        RETURNING id::INTEGER
        """
    )

    assert columns is not None
    assert len(columns) == 1
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[0].sql_type == "INT"


def test_merge_returning_multiple_columns(analyzer: SQLAnalyzer) -> None:
    """Test MERGE with RETURNING multiple columns."""
    columns = analyzer.analyze_select(
        """
        MERGE INTO users
        USING updates ON users.id = updates.id
        WHEN MATCHED THEN UPDATE SET name = updates.name, email = updates.email
        WHEN NOT MATCHED THEN INSERT (name, email) VALUES (updates.name, updates.email)
        RETURNING id::INTEGER, name::TEXT, email::TEXT
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[1].name == "name"
    assert columns[1].python_type == "str"
    assert columns[2].name == "email"
    assert columns[2].python_type == "str"


def test_merge_returning_generate_type(analyzer: SQLAnalyzer) -> None:
    """Test generate_return_type for MERGE with RETURNING."""
    return_type = analyzer.generate_return_type(
        """
        MERGE INTO users
        USING updates ON users.id = updates.id
        WHEN MATCHED THEN UPDATE SET name = updates.name
        RETURNING id::INTEGER, name::TEXT
        """
    )

    assert return_type == "tuple[int, str]"


# Edge cases and complex scenarios


def test_update_returning_without_where(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE without WHERE clause with RETURNING."""
    columns = analyzer.analyze_select(
        "UPDATE users SET active = false RETURNING id::INTEGER, active::BOOLEAN"
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "bool"


def test_delete_returning_all(analyzer: SQLAnalyzer) -> None:
    """Test DELETE with RETURNING *."""
    columns = analyzer.analyze_select("DELETE FROM users WHERE active = false RETURNING *")

    assert columns is not None
    # RETURNING * is treated as a single projection expression by sqlglot.
    # See test_update_returning_all_columns for detailed explanation.
    assert len(columns) == 1


def test_update_returning_with_expressions(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with RETURNING expressions."""
    columns = analyzer.analyze_select(
        """
        UPDATE products
        SET price = price * 1.1
        RETURNING id::INTEGER, (price * 1.1)::FLOAT
        """
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "float"


def test_returning_with_datetime_types(analyzer: SQLAnalyzer) -> None:
    """Test RETURNING with various datetime types."""
    columns = analyzer.analyze_select(
        """
        UPDATE events
        SET status = 'completed'
        RETURNING
            id::INTEGER,
            created_at::TIMESTAMP,
            completed_at::TIMESTAMPTZ,
            scheduled_date::DATE,
            scheduled_time::TIME
        """
    )

    assert columns is not None
    assert len(columns) == 5
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "datetime.datetime"
    assert columns[2].python_type == "datetime.datetime"
    assert columns[3].python_type == "datetime.date"
    assert columns[4].python_type == "datetime.time"


def test_returning_with_uuid(analyzer: SQLAnalyzer) -> None:
    """Test RETURNING with UUID type."""
    columns = analyzer.analyze_select(
        "INSERT INTO users (name) VALUES ('John') RETURNING user_id::UUID, name::TEXT"
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "uuid.UUID"
    assert columns[1].python_type == "str"


def test_returning_with_json_types(analyzer: SQLAnalyzer) -> None:
    """Test RETURNING with JSON types."""
    columns = analyzer.analyze_select(
        """
        UPDATE settings
        SET config = '{"key": "value"}'::jsonb
        RETURNING id::INTEGER, config::JSONB
        """
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "dict"


def test_update_with_from_clause_returning(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with FROM clause and RETURNING."""
    columns = analyzer.analyze_select(
        """
        UPDATE users
        SET status = 'active'
        FROM orders
        WHERE users.id = orders.user_id AND orders.total > 100
        RETURNING users.id::INTEGER, users.status::TEXT
        """
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"


def test_delete_with_using_clause_returning(analyzer: SQLAnalyzer) -> None:
    """Test DELETE with USING clause and RETURNING."""
    columns = analyzer.analyze_select(
        """
        DELETE FROM orders
        USING users
        WHERE orders.user_id = users.id AND users.active = false
        RETURNING orders.id::INTEGER, orders.total::FLOAT
        """
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "float"


# Backward compatibility tests


def test_regular_select_still_works(analyzer: SQLAnalyzer) -> None:
    """Test that regular SELECT queries still work."""
    columns = analyzer.analyze_select("SELECT id::INTEGER, name::TEXT FROM users")

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"


def test_update_without_returning_returns_none(analyzer: SQLAnalyzer) -> None:
    """Test that UPDATE without RETURNING returns None."""
    columns = analyzer.analyze_select("UPDATE users SET name = 'John' WHERE id = 1")

    assert columns is None


def test_delete_without_returning_returns_none(analyzer: SQLAnalyzer) -> None:
    """Test that DELETE without RETURNING returns None."""
    columns = analyzer.analyze_select("DELETE FROM users WHERE id = 1")

    assert columns is None


def test_insert_without_returning_returns_none(analyzer: SQLAnalyzer) -> None:
    """Test that INSERT without RETURNING returns None."""
    columns = analyzer.analyze_select("INSERT INTO users (name) VALUES ('John')")

    assert columns is None


# Edge cases and corner cases


def test_update_from_returning_different_tables(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE...FROM where RETURNING references columns from both tables."""
    columns = analyzer.analyze_select(
        """
        UPDATE users
        SET status = orders.status
        FROM orders
        WHERE users.id = orders.user_id
        RETURNING users.id::INTEGER, orders.order_id::INTEGER, users.status::TEXT
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].name == "id"
    assert columns[0].python_type == "int"
    assert columns[1].name == "order_id"
    assert columns[1].python_type == "int"
    assert columns[2].name == "status"
    assert columns[2].python_type == "str"


def test_delete_using_returning_different_tables(analyzer: SQLAnalyzer) -> None:
    """Test DELETE...USING where RETURNING references columns from both tables."""
    columns = analyzer.analyze_select(
        """
        DELETE FROM orders
        USING users, products
        WHERE orders.user_id = users.id
        AND orders.product_id = products.id
        AND users.active = false
        RETURNING orders.id::INTEGER, users.email::TEXT, products.name::TEXT
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"
    assert columns[2].python_type == "str"


def test_update_with_subquery_in_set_returning(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with subquery in SET clause and RETURNING."""
    columns = analyzer.analyze_select(
        """
        UPDATE users
        SET total_orders = (SELECT COUNT(*) FROM orders WHERE orders.user_id = users.id)
        WHERE active = true
        RETURNING id::INTEGER, total_orders::INTEGER
        """
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "int"


def test_update_from_join_returning(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with JOIN in FROM clause and RETURNING."""
    columns = analyzer.analyze_select(
        """
        UPDATE users
        SET tier = 'premium'
        FROM (
            SELECT user_id, SUM(total) as total_spent
            FROM orders
            GROUP BY user_id
            HAVING SUM(total) > 1000
        ) AS high_spenders
        WHERE users.id = high_spenders.user_id
        RETURNING users.id::INTEGER, users.tier::TEXT, high_spenders.total_spent::FLOAT
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"
    assert columns[2].python_type == "float"


def test_delete_with_cte_returning(analyzer: SQLAnalyzer) -> None:
    """Test DELETE with CTE (WITH clause) and RETURNING."""
    columns = analyzer.analyze_select(
        """
        WITH inactive_users AS (
            SELECT id FROM users WHERE last_login < NOW() - INTERVAL '1 year'
        )
        DELETE FROM orders
        WHERE user_id IN (SELECT id FROM inactive_users)
        RETURNING id::INTEGER, user_id::INTEGER, created_at::TIMESTAMP
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "int"
    assert columns[2].python_type == "datetime.datetime"


def test_update_returning_with_expression(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE RETURNING with expressions and calculations."""
    columns = analyzer.analyze_select(
        """
        UPDATE products
        SET price = price * 1.1, updated_at = NOW()
        RETURNING id::INTEGER, (price * 1.1)::FLOAT AS new_price, UPPER(name)::TEXT AS upper_name
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "float"
    assert columns[2].python_type == "str"


def test_insert_select_returning(analyzer: SQLAnalyzer) -> None:
    """Test INSERT...SELECT with RETURNING."""
    columns = analyzer.analyze_select(
        """
        INSERT INTO audit_log (user_id, action, timestamp)
        SELECT id, 'login', NOW()
        FROM users
        WHERE active = true
        RETURNING id::INTEGER, user_id::INTEGER, action::TEXT, timestamp::TIMESTAMP
        """
    )

    assert columns is not None
    assert len(columns) == 4
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "int"
    assert columns[2].python_type == "str"
    assert columns[3].python_type == "datetime.datetime"


def test_update_from_multiple_joins_returning(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with multiple JOINs in FROM and RETURNING."""
    columns = analyzer.analyze_select(
        """
        UPDATE order_items
        SET discount = discounts.percentage
        FROM orders
        INNER JOIN users ON orders.user_id = users.id
        INNER JOIN discounts ON users.tier = discounts.tier
        WHERE order_items.order_id = orders.id
        RETURNING order_items.id::INTEGER, order_items.discount::FLOAT, users.tier::TEXT
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "float"
    assert columns[2].python_type == "str"


def test_delete_from_nested_select_returning(analyzer: SQLAnalyzer) -> None:
    """Test DELETE with nested SELECT in WHERE and RETURNING."""
    columns = analyzer.analyze_select(
        """
        DELETE FROM users
        WHERE id IN (
            SELECT user_id FROM orders
            WHERE status = 'cancelled'
            AND created_at < (
                SELECT MIN(created_at) FROM orders WHERE status = 'completed'
            )
        )
        RETURNING id::INTEGER, email::TEXT, created_at::TIMESTAMP
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"
    assert columns[2].python_type == "datetime.datetime"


def test_update_returning_with_case_expression(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE RETURNING with CASE expression."""
    columns = analyzer.analyze_select(
        """
        UPDATE users
        SET tier = CASE
            WHEN total_spent > 10000 THEN 'platinum'
            WHEN total_spent > 5000 THEN 'gold'
            WHEN total_spent > 1000 THEN 'silver'
            ELSE 'bronze'
        END
        RETURNING id::INTEGER, tier::TEXT, total_spent::FLOAT
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"
    assert columns[2].python_type == "float"


def test_merge_with_complex_conditions_returning(analyzer: SQLAnalyzer) -> None:
    """Test MERGE with complex WHEN conditions and RETURNING."""
    columns = analyzer.analyze_select(
        """
        MERGE INTO inventory
        USING (
            SELECT product_id, SUM(quantity) as qty
            FROM order_items
            GROUP BY product_id
        ) AS orders
        ON inventory.product_id = orders.product_id
        WHEN MATCHED AND orders.qty > 0 THEN
            UPDATE SET quantity = inventory.quantity - orders.qty
        WHEN NOT MATCHED THEN
            INSERT (product_id, quantity) VALUES (orders.product_id, 0)
        RETURNING inventory.product_id::INTEGER, inventory.quantity::INTEGER, orders.qty::INTEGER
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "int"
    assert columns[2].python_type == "int"


def test_update_from_lateral_join_returning(analyzer: SQLAnalyzer) -> None:
    """Test UPDATE with LATERAL join in FROM and RETURNING."""
    columns = analyzer.analyze_select(
        """
        UPDATE users
        SET last_order_date = latest.order_date
        FROM LATERAL (
            SELECT MAX(created_at) as order_date
            FROM orders
            WHERE orders.user_id = users.id
        ) AS latest
        WHERE users.active = true
        RETURNING users.id::INTEGER, users.last_order_date::DATE
        """
    )

    assert columns is not None
    assert len(columns) == 2
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "datetime.date"


def test_delete_returning_with_window_function_expression(analyzer: SQLAnalyzer) -> None:
    """Test DELETE RETURNING with window function in expression."""
    columns = analyzer.analyze_select(
        """
        DELETE FROM duplicates
        WHERE id NOT IN (
            SELECT id FROM (
                SELECT id, ROW_NUMBER() OVER (PARTITION BY email ORDER BY created_at DESC) as rn
                FROM duplicates
            ) t WHERE rn = 1
        )
        RETURNING id::INTEGER, email::TEXT, created_at::TIMESTAMP
        """
    )

    assert columns is not None
    assert len(columns) == 3
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"
    assert columns[2].python_type == "datetime.datetime"


def test_insert_on_conflict_returning(analyzer: SQLAnalyzer) -> None:
    """Test INSERT...ON CONFLICT with RETURNING."""
    columns = analyzer.analyze_select(
        """
        INSERT INTO users (email, name, login_count)
        VALUES ('user@example.com', 'User', 1)
        ON CONFLICT (email) DO UPDATE
        SET login_count = users.login_count + 1, last_login = NOW()
        RETURNING id::INTEGER, email::TEXT, login_count::INTEGER, last_login::TIMESTAMP
        """
    )

    assert columns is not None
    assert len(columns) == 4
    assert columns[0].python_type == "int"
    assert columns[1].python_type == "str"
    assert columns[2].python_type == "int"
    assert columns[3].python_type == "datetime.datetime"
