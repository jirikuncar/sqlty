"""The engine caches parsed schemas and skips them for sources without queries."""

from sqlty import core


def _schema(i: int) -> str:
    return f"CREATE TABLE t (c{i} INTEGER NOT NULL);"


def test_alternating_and_many_schemas_stay_distinct() -> None:
    # more schemas than the cache keeps, revisited out of order
    for i in [*range(20), *reversed(range(20)), 3, 3, 17, 0]:
        assert core.analyze_select("SELECT * FROM t", schema_sql=_schema(i)) == [
            (f"c{i}", "int", "INT")
        ]


def test_dialect_is_part_of_the_key() -> None:
    sql = "CREATE TABLE t (a INT3 ZEROFILL);"
    assert core.analyze_select("SELECT a FROM t", dialect="mysql", schema_sql=sql) != (
        core.analyze_select("SELECT a FROM t", dialect="postgres", schema_sql=sql)
    )


def test_source_without_queries_is_unchanged_by_schema() -> None:
    source = "def f():\n    return 1\n"
    schema = _schema(0)
    assert core.check_source(source, schema_sql=schema) == core.check_source(source)
    assert core.rewrite_source(source, schema_sql=schema) == source
    assert core.fix_source(source, schema_sql=schema) == source


def test_source_with_a_query_uses_the_schema() -> None:
    source = 'q = "--sql\\nSELECT * FROM t"\n'
    assert "SQL[tuple[int]]" in core.rewrite_source(source, schema_sql=_schema(0))


def test_sources_skipped_for_lack_of_sql_have_no_schema_diagnostics() -> None:
    # the shortcut keys on the text `sql`; a wrapped query without a marker is never inferred
    style = "from sqlty import SQL as Q"
    cases = [
        ('q = Q[tuple[str]]("--sql\\nSELECT c0 FROM t")\n', {"import_style": style}),
        ('q = "--sql\\nSELECT * FROM t"\n', {"dialect": "mysql"}),
        ('q = "-- sql\\nSELECT c0 FROM t"\n', {}),
        ('q = "--SQL\\nSELECT c0 FROM t"\n', {}),
        (
            'q = sy.SQL[tuple[str]]("--sql\\nSELECT c0 FROM t")\n',
            {"import_style": "import sqlty as sy"},
        ),
        ('q = Q[tuple[str]]("SELECT c0 FROM t")\n', {"import_style": style}),
    ]
    for source, kwargs in cases:
        if "sql" not in source.lower():
            assert core.check_source(source, schema_sql=_schema(0), **kwargs) == (
                core.check_source(source, **kwargs)
            )


def test_a_loaded_schema_stands_in_for_its_text() -> None:
    sql = "CREATE TABLE t (a INT3 ZEROFILL NOT NULL); CREATE TABLE bad (x INT,;"
    loaded = core.load_schema(sql, dialect="mysql")
    assert loaded.dialect == "mysql"
    query = "SELECT a FROM t"
    assert core.analyze_select(query, dialect="mysql", schema_sql=loaded) == (
        core.analyze_select(query, dialect="mysql", schema_sql=sql)
    )
    assert core.schema_skipped_objects(loaded, dialect="mysql") == [("table", "bad")]
    source = 'q = "--sql\\nSELECT a FROM t"\n'
    assert core.rewrite_source(source, dialect="mysql", schema_sql=loaded) == (
        core.rewrite_source(source, dialect="mysql", schema_sql=sql)
    )


def test_a_loaded_schema_is_parsed_again_for_another_dialect() -> None:
    sql = "CREATE TABLE t (a INT3 ZEROFILL);"
    loaded = core.load_schema(sql, dialect="mysql")
    assert core.analyze_select("SELECT a FROM t", dialect="postgres", schema_sql=loaded) == (
        core.analyze_select("SELECT a FROM t", dialect="postgres", schema_sql=sql)
    )
