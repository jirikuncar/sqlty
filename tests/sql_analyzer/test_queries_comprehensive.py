"""Test comprehensive SQL query patterns including JSON aggregations."""

import pytest

from sqlty._sql_analyzer import SQLAnalyzer


@pytest.fixture
def analyzer():
    """Create SQL analyzer instance."""
    return SQLAnalyzer()


# Basic Queries Tests


def test_simple_select(analyzer: SQLAnalyzer) -> None:
    """Test simple SELECT query."""
    result = analyzer.generate_return_type("SELECT id::INTEGER, name::TEXT FROM users")
    assert result == "tuple[int, str]"


def test_select_with_where(analyzer: SQLAnalyzer) -> None:
    """Test SELECT with WHERE clause."""
    result = analyzer.generate_return_type(
        "SELECT id::INTEGER, email::TEXT FROM users WHERE active::BOOLEAN = true"
    )
    assert result == "tuple[int, str]"


def test_select_with_joins(analyzer: SQLAnalyzer) -> None:
    """Test SELECT with JOINs."""
    result = analyzer.generate_return_type("""
        SELECT
            u.id::INTEGER,
            u.name::TEXT,
            p.title::TEXT
        FROM users u
        JOIN posts p ON u.id = p.user_id
    """)
    assert result == "tuple[int, str, str]"


def test_select_with_multiple_joins(analyzer: SQLAnalyzer) -> None:
    """Test SELECT with multiple JOINs."""
    result = analyzer.generate_return_type("""
        SELECT
            u.id::INTEGER,
            u.name::TEXT,
            p.title::TEXT,
            c.content::TEXT
        FROM users u
        JOIN posts p ON u.id = p.user_id
        JOIN comments c ON p.id = c.post_id
    """)
    assert result == "tuple[int, str, str, str]"


# Aggregation Tests


def test_count(analyzer: SQLAnalyzer) -> None:
    """Test COUNT aggregation."""
    result = analyzer.generate_return_type("SELECT COUNT(*)::INTEGER FROM users")
    assert result == "tuple[int]"


def test_sum(analyzer: SQLAnalyzer) -> None:
    """Test SUM aggregation."""
    result = analyzer.generate_return_type("SELECT SUM(price)::FLOAT FROM products")
    assert result == "tuple[float]"


def test_avg(analyzer: SQLAnalyzer) -> None:
    """Test AVG aggregation."""
    result = analyzer.generate_return_type("SELECT AVG(score)::FLOAT FROM reviews")
    assert result == "tuple[float]"


def test_min_max(analyzer: SQLAnalyzer) -> None:
    """Test MIN and MAX aggregations."""
    result = analyzer.generate_return_type("""
        SELECT
            MIN(price)::FLOAT,
            MAX(price)::FLOAT
        FROM products
    """)
    assert result == "tuple[float, float]"


def test_group_by_with_aggregations(analyzer: SQLAnalyzer) -> None:
    """Test GROUP BY with aggregations."""
    result = analyzer.generate_return_type("""
        SELECT
            category::TEXT,
            COUNT(*)::INTEGER,
            AVG(price)::FLOAT
        FROM products
        GROUP BY category
    """)
    assert result == "tuple[str, int, float]"


def test_having_clause(analyzer: SQLAnalyzer) -> None:
    """Test HAVING clause with aggregations."""
    result = analyzer.generate_return_type("""
        SELECT
            user_id::INTEGER,
            COUNT(*)::INTEGER
        FROM posts
        GROUP BY user_id
        HAVING COUNT(*) > 10
    """)
    assert result == "tuple[int, int]"


# JSON Queries Tests


def test_json_field_extraction(analyzer: SQLAnalyzer) -> None:
    """Test JSON field extraction."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            (data->>'name')::TEXT
        FROM users
    """)
    assert result == "tuple[int, str]"


def test_json_nested_extraction(analyzer: SQLAnalyzer) -> None:
    """Test nested JSON extraction."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            (data->'profile'->>'email')::TEXT
        FROM users
    """)
    assert result == "tuple[int, str]"


def test_jsonb_agg(analyzer: SQLAnalyzer) -> None:
    """Test jsonb_agg aggregation."""
    result = analyzer.generate_return_type("""
        SELECT
            category::TEXT,
            jsonb_agg(name)::TEXT
        FROM products
        GROUP BY category
    """)
    assert result == "tuple[str, str]"


def test_json_build_object(analyzer: SQLAnalyzer) -> None:
    """Test json_build_object."""
    result = analyzer.generate_return_type("""
        SELECT
            json_build_object('id', id, 'name', name)::TEXT
        FROM users
    """)
    assert result == "tuple[str]"


def test_json_nested_aggregation(analyzer: SQLAnalyzer) -> None:
    """Test nested JSON aggregation with explicit cast."""
    result = analyzer.generate_return_type("""
        SELECT
            u.id::INTEGER,
            u.name::TEXT,
            COALESCE(jsonb_agg(p.title), '[]'::jsonb)::TEXT
        FROM users u
        LEFT JOIN posts p ON u.id = p.user_id
        GROUP BY u.id, u.name
    """)
    assert result == "tuple[int, str, str]"


# Complex Types Tests


def test_date_time_types(analyzer: SQLAnalyzer) -> None:
    """Test date and time types."""
    result = analyzer.generate_return_type("""
        SELECT
            created_date::DATE,
            created_at::TIMESTAMP,
            updated_at::TIMESTAMPTZ,
            scheduled_time::TIME
        FROM events
    """)
    # Date/time types are now mapped to datetime module types
    assert result == "tuple[datetime.date, datetime.datetime, datetime.datetime, datetime.time]"


def test_numeric_types(analyzer: SQLAnalyzer) -> None:
    """Test various numeric types."""
    result = analyzer.generate_return_type("""
        SELECT
            tiny::SMALLINT,
            normal::INTEGER,
            big::BIGINT,
            amount::DECIMAL,
            ratio::FLOAT,
            precise::DOUBLE
        FROM measurements
    """)
    # DECIMAL without explicit cast function returns float
    assert result == "tuple[int, int, int, float, float, float]"


def test_binary_types(analyzer: SQLAnalyzer) -> None:
    """Test binary types."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            document::BYTEA,
            image::BLOB
        FROM files
    """)
    # Binary types without explicit cast function return str
    assert result == "tuple[int, str, str]"


def test_boolean_type(analyzer: SQLAnalyzer) -> None:
    """Test BOOLEAN type."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            is_active::BOOLEAN,
            is_verified::BOOL
        FROM users
    """)
    assert result == "tuple[int, bool, bool]"


# Subqueries Tests


def test_subquery_in_select(analyzer: SQLAnalyzer) -> None:
    """Test subquery in SELECT clause."""
    result = analyzer.generate_return_type("""
        SELECT
            u.id::INTEGER,
            u.name::TEXT,
            (SELECT COUNT(*)::INTEGER FROM posts WHERE user_id = u.id)
        FROM users u
    """)
    # Subquery expressions without explicit cast function return str
    assert result == "tuple[int, str, str]"


def test_cte_simple(analyzer: SQLAnalyzer) -> None:
    """Test simple CTE (Common Table Expression)."""
    result = analyzer.generate_return_type("""
        WITH active_users AS (
            SELECT id, name FROM users WHERE active = true
        )
        SELECT
            id::INTEGER,
            name::TEXT
        FROM active_users
    """)
    assert result == "tuple[int, str]"


def test_cte_multiple(analyzer: SQLAnalyzer) -> None:
    """Test multiple CTEs."""
    result = analyzer.generate_return_type("""
        WITH
            active_users AS (
                SELECT id, name FROM users WHERE active = true
            ),
            user_posts AS (
                SELECT user_id, COUNT(*) as count FROM posts GROUP BY user_id
            )
        SELECT
            u.id::INTEGER,
            u.name::TEXT,
            COALESCE(p.count, 0)::INTEGER
        FROM active_users u
        LEFT JOIN user_posts p ON u.id = p.user_id
    """)
    assert result == "tuple[int, str, int]"


# Window Functions Tests


def test_row_number(analyzer: SQLAnalyzer) -> None:
    """Test ROW_NUMBER window function."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            name::TEXT,
            ROW_NUMBER() OVER (ORDER BY created_at)::INTEGER
        FROM users
    """)
    assert result == "tuple[int, str, int]"


def test_rank_dense_rank(analyzer: SQLAnalyzer) -> None:
    """Test RANK and DENSE_RANK."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            score::FLOAT,
            RANK() OVER (ORDER BY score DESC)::INTEGER,
            DENSE_RANK() OVER (ORDER BY score DESC)::INTEGER
        FROM scores
    """)
    assert result == "tuple[int, float, int, int]"


def test_window_with_partition(analyzer: SQLAnalyzer) -> None:
    """Test window function with PARTITION BY."""
    result = analyzer.generate_return_type("""
        SELECT
            user_id::INTEGER,
            post_id::INTEGER,
            ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY created_at)::INTEGER
        FROM posts
    """)
    assert result == "tuple[int, int, int]"


# Union and Set Operations Tests


def test_union(analyzer: SQLAnalyzer) -> None:
    """Test UNION."""
    result = analyzer.generate_return_type("""
        SELECT id::INTEGER, name::TEXT FROM users
        UNION
        SELECT id::INTEGER, name::TEXT FROM customers
    """)
    assert result == "tuple[int, str]"


def test_union_all(analyzer: SQLAnalyzer) -> None:
    """Test UNION ALL."""
    result = analyzer.generate_return_type("""
        SELECT id::INTEGER, email::TEXT FROM users
        UNION ALL
        SELECT id::INTEGER, email::TEXT FROM subscribers
    """)
    assert result == "tuple[int, str]"


def test_intersect(analyzer: SQLAnalyzer) -> None:
    """Test INTERSECT."""
    result = analyzer.generate_return_type("""
        SELECT id::INTEGER FROM active_users
        INTERSECT
        SELECT id::INTEGER FROM verified_users
    """)
    assert result == "tuple[int]"


def test_except(analyzer: SQLAnalyzer) -> None:
    """Test EXCEPT."""
    result = analyzer.generate_return_type("""
        SELECT id::INTEGER, name::TEXT FROM all_users
        EXCEPT
        SELECT id::INTEGER, name::TEXT FROM deleted_users
    """)
    assert result == "tuple[int, str]"


# Case Functions Tests


def test_case_simple(analyzer: SQLAnalyzer) -> None:
    """Test simple CASE expression."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            CASE
                WHEN score > 90 THEN 'A'
                WHEN score > 80 THEN 'B'
                ELSE 'C'
            END::TEXT
        FROM students
    """)
    assert result == "tuple[int, str]"


def test_case_with_aggregation(analyzer: SQLAnalyzer) -> None:
    """Test CASE with aggregation."""
    result = analyzer.generate_return_type("""
        SELECT
            category::TEXT,
            SUM(CASE WHEN active THEN 1 ELSE 0 END)::INTEGER
        FROM products
        GROUP BY category
    """)
    assert result == "tuple[str, int]"


def test_coalesce(analyzer: SQLAnalyzer) -> None:
    """Test COALESCE function."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            COALESCE(nickname, username)::TEXT,
            COALESCE(balance, 0.0)::FLOAT
        FROM users
    """)
    assert result == "tuple[int, str, float]"


def test_nullif(analyzer: SQLAnalyzer) -> None:
    """Test NULLIF function."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            NULLIF(status, 'deleted')::TEXT
        FROM records
    """)
    assert result == "tuple[int, str]"


# String Functions Tests


def test_concat(analyzer: SQLAnalyzer) -> None:
    """Test CONCAT function."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            CONCAT(first_name, ' ', last_name)::TEXT
        FROM users
    """)
    assert result == "tuple[int, str]"


def test_substring(analyzer: SQLAnalyzer) -> None:
    """Test SUBSTRING function."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            SUBSTRING(email FROM 1 FOR 10)::TEXT
        FROM users
    """)
    assert result == "tuple[int, str]"


def test_string_agg(analyzer: SQLAnalyzer) -> None:
    """Test STRING_AGG function."""
    result = analyzer.generate_return_type("""
        SELECT
            user_id::INTEGER,
            STRING_AGG(tag, ', ')::TEXT
        FROM user_tags
        GROUP BY user_id
    """)
    assert result == "tuple[int, str]"


# Edge Cases Tests


def test_select_star_no_cast(analyzer: SQLAnalyzer) -> None:
    """Test SELECT * without type casts."""
    result = analyzer.generate_return_type("SELECT * FROM users")
    # SELECT * returns a simple tuple[str]
    assert result == "tuple[str]"


def test_select_without_from(analyzer: SQLAnalyzer) -> None:
    """Test SELECT without FROM clause."""
    result = analyzer.generate_return_type("SELECT 1::INTEGER, 'hello'::TEXT")
    assert result == "tuple[int, str]"


def test_select_with_distinct(analyzer: SQLAnalyzer) -> None:
    """Test SELECT DISTINCT."""
    result = analyzer.generate_return_type("""
        SELECT DISTINCT
            category::TEXT,
            status::TEXT
        FROM products
    """)
    assert result == "tuple[str, str]"


def test_select_with_limit_offset(analyzer: SQLAnalyzer) -> None:
    """Test SELECT with LIMIT and OFFSET."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            name::TEXT
        FROM users
        ORDER BY name
        LIMIT 10 OFFSET 20
    """)
    assert result == "tuple[int, str]"


def test_exists_subquery(analyzer: SQLAnalyzer) -> None:
    """Test EXISTS subquery."""
    result = analyzer.generate_return_type("""
        SELECT
            u.id::INTEGER,
            u.name::TEXT
        FROM users u
        WHERE EXISTS (
            SELECT 1 FROM posts WHERE user_id = u.id
        )
    """)
    assert result == "tuple[int, str]"


def test_in_subquery(analyzer: SQLAnalyzer) -> None:
    """Test IN subquery."""
    result = analyzer.generate_return_type("""
        SELECT
            id::INTEGER,
            name::TEXT
        FROM users
        WHERE id IN (SELECT user_id FROM active_sessions)
    """)
    assert result == "tuple[int, str]"
