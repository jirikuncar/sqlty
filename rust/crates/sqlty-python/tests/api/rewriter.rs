use crate::common::*;
use sqlty_python::options::Options;
use sqlty_python::rewriter::rewrite_source_with;
use sqlty_sql::analyzer::SchemaData;

fn rewrite_source(
    source: &str,
    dialect: &str,
    schema: Option<&SchemaData>,
    style: Option<&str>,
    untyped: bool,
) -> String {
    let options = Options {
        dialect: dialect.parse().unwrap(),
        schema,
        style: style_or_default(style),
        untyped,
    };
    rewrite_source_with(source, &options, false)
}

fn strip_source(source: &str, style: Option<&str>) -> String {
    sqlty_python::rewriter::strip_source(source, &style_or_default(style))
}

#[test]
fn test_rewrite_basic() {
    let result = rewrite_source(
        r#"q = "--sql\nSELECT 1::INTEGER""#,
        "postgres",
        None,
        None,
        false,
    );
    assert!(result.contains("SQL[tuple[int]]"));
    assert!(result.contains("from sqlty import SQL"));
}

#[test]
fn test_rewrite_preserves_string_content() {
    // Rewriter never modifies string content — only adds SQL[T]() wrapper
    let result = rewrite_source(
        r#"q = "--sql\nSELECT 1::INTEGER""#,
        "postgres",
        None,
        None,
        false,
    );
    assert!(result.contains(r#""--sql\nSELECT 1::INTEGER""#));
}

#[test]
fn test_space_form_is_sql_comment_not_wrapped() {
    // --sql SELECT ... with space is a SQL comment — parser sees empty query
    let result = rewrite_source(
        r#"q = "--sql SELECT 1::INTEGER""#,
        "postgres",
        None,
        None,
        false,
    );
    assert!(!result.contains("SQL["));
}

#[test]
fn test_strip_basic() {
    let source = r#"from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
    let result = strip_source(source, None);
    assert!(!result.contains("SQL["));
    assert!(result.contains(r#""--sql\nSELECT 1::INTEGER""#));
}

#[test]
fn test_rewrite_idempotent() {
    let source = r#"q = "--sql\nSELECT 1::INTEGER""#;
    let once = rewrite_source(source, "postgres", None, None, false);
    let twice = rewrite_source(&once, "postgres", None, None, false);
    assert_eq!(once, twice);
}

#[test]
fn test_strip_rewrite_roundtrip() {
    // strip(rewrite(source)) == source (ignoring imports)
    let original = r#"q = "--sql\nSELECT 1::INTEGER""#;
    let rewritten = rewrite_source(original, "postgres", None, None, false);
    let stripped = strip_source(&rewritten, None);
    assert_eq!(stripped.trim(), original.trim());
}

#[test]
fn test_concatenated_strings_roundtrip() {
    let source = "q = (\"--sql\\nSELECT 1::INTEGER \"  # c\n     'AS n')\n";
    let rewritten = rewrite_source(source, "postgres", None, None, false);
    assert_eq!(
        rewritten,
        "from sqlty import SQL\nq = (SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER \"  # c\n     'AS n'))\n"
    );
    assert_eq!(strip_source(&rewritten, None), source);
}

#[test]
fn test_lexing_gaps_keep_statements_apart() {
    let query = "q = \"--sql\\nSELECT 1::INTEGER AS n\"\n\"\"\"Attribute doc.\"\"\"\n";
    let wrapped =
        "q = SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER AS n\")\n\"\"\"Attribute doc.\"\"\"\n";
    let call = "q = SQL[tuple[int]](\n    \"--sql\\nSELECT 1::INTEGER \"\n    \"AS n\"\n)\n";
    let stripped = "q = (\n    \"--sql\\nSELECT 1::INTEGER \"\n    \"AS n\"\n)\n";
    for prefix in [
        "x = f\"{\"(\"}\"\n",
        "x = 1 if\"(\" else 2\n",
        "def f():\n    return\"(\"\n",
        "x = \"a\\\r\nb(\"\n",
    ] {
        let rewritten = rewrite_source(&format!("{prefix}{query}"), "postgres", None, None, false);
        assert!(
            rewritten.ends_with(&format!("{prefix}{wrapped}")),
            "{rewritten}"
        );
        assert_eq!(
            strip_source(&format!("{prefix}{call}"), None),
            format!("{prefix}{stripped}")
        );
    }
}

#[test]
fn only_top_level_imports_of_sqlty_bind_the_wrapper() {
    // Whether `from sqlty import SQL` is added tells which imports the rewrite found.
    let added = |imports: &str| {
        let source = format!("{imports}q = \"--sql\\nSELECT 1::INTEGER\"\n");
        let rewritten = rewrite_source(&source, "postgres", None, None, false);
        rewritten.matches("from sqlty import").count() > source.matches("from sqlty import").count()
    };
    for found in [
        "from sqlty import SQL, UNTYPED  # c\n",
        "from sqlty import (\n    A,  # )\n    SQL,\n)\n",
        "x = 1; from sqlty import *\n",
    ] {
        assert!(!added(found), "{found}");
    }
    for missed in [
        "from sqlty import SQL as Q, UNTYPED  # c\n",
        "from sqlty import (\n    A,  # )\n    B as C,\n)\n",
        "x = 1\nif x: pass; from sqlty import SQL\n",
        "if x:\n    pass\nelse: pass; from sqlty import SQL\n",
        "if x:\n    from sqlty import SQL\n",
        "'''\nfrom sqlty import SQL\n'''\n",
    ] {
        assert!(added(missed), "{missed}");
    }
}

#[test]
fn test_imports_after_a_long_line() {
    // Each token used to scan back to its line's start: quadratic in the line's length.
    let long = ["1"; 50_000].join("+");
    let source = format!(
        "x = {long}\nfrom sqlty import SQL\nx = {long}; from sqlty import A\nq = \"--sql\\nSELECT 1::INTEGER\"\n"
    );
    let rewritten = rewrite_source(&source, "postgres", None, None, false);
    assert!(rewritten.ends_with("q = SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER\")\n"));
    assert_eq!(rewritten.matches("from sqlty import").count(), 2);
}

#[test]
fn test_import_in_compound_statement_line_is_not_top_level() {
    let source = "from typing import TYPE_CHECKING\nif TYPE_CHECKING: pass; from sqlty import SQL\nq = \"--sql\\nSELECT 1::INTEGER\"\n";
    let rewritten = rewrite_source(source, "postgres", None, None, false);
    assert!(
        rewritten.starts_with("from sqlty import SQL\nfrom typing"),
        "{rewritten}"
    );
    assert_eq!(strip_source(&rewritten, None), source);
}

#[test]
fn test_crlf_inserted_lines() {
    for (source, expected) in [
        (
            "q = \"--sql\\nSELECT NOW()::TIMESTAMP\"\r\n",
            "import datetime\r\nfrom sqlty import SQL\r\nq = SQL[",
        ),
        (
            "\"\"\"Doc.\"\"\"\r\nq = \"--sql\\nSELECT 1::INTEGER\"",
            "\"\"\"Doc.\"\"\"\r\nfrom sqlty import SQL\r\nq = SQL[",
        ),
        (
            "\"\"\"Doc.\"\"\"\r\nq = \"--sql\\nSELECT 1::INTEGER\"\r\n",
            "\"\"\"Doc.\"\"\"\r\nfrom sqlty import SQL\r\nq = SQL[",
        ),
    ] {
        let rewritten = rewrite_source(source, "postgres", None, None, false);
        assert!(rewritten.starts_with(expected), "{rewritten:?}");
        assert_eq!(strip_source(&rewritten, None), source);
    }
    let compound = keep_lines("def f():\r\n    pass\r\nq = \"--sql\\nSELECT 1::INTEGER\"\r\n");
    assert!(
        compound.starts_with("from sqlty import SQL\r\ndef f():"),
        "{compound:?}"
    );
}

#[test]
fn test_existing_sqlty_imports_round_trip() {
    let imports = |text: &str| text.matches("from sqlty import").count();
    for (source, untyped, added) in [
        ("from sqlty import SQL, UNTYPED\n", true, 0),
        ("from sqlty import *\n", true, 0),
        ("from sqlty import (\n    SQL,  # c\n)\n", false, 0),
        ("from sqlty import SQL as S\n", false, 1),
        ("from sqlty import (\n    UNTYPED,  # c\n)\n", false, 1),
        ("from sqlty import UNTYPED; x = 1\n", false, 1),
        ("\"\"\"from sqlty import SQL\"\"\"\n", false, 1),
    ] {
        let query = if untyped {
            "SELECT x, y FROM pairs"
        } else {
            "SELECT 1::INTEGER"
        };
        let original = format!("{source}q = \"--sql\\n{query}\"\n");
        let rewritten = rewrite_source(&original, "postgres", None, None, untyped);
        assert_eq!(
            imports(&rewritten),
            imports(&original) + added,
            "{rewritten}"
        );
        assert_eq!(strip_source(&rewritten, None), original, "{rewritten}");
    }
}

#[test]
fn test_non_sql_unchanged() {
    let source = "x = 42\nname = \"hello\"";
    assert_eq!(
        rewrite_source(source, "postgres", None, None, false),
        source
    );
}

#[test]
fn test_strip_preserves_manual() {
    let source = r#"q = SQL[int]("SELECT 1")"#;
    assert_eq!(strip_source(source, None), source);
}

#[test]
fn strip_unwraps_only_marked_queries() {
    for managed in [
        "--sql SELECT 1",
        "--sql\\nSELECT 1",
        "--sql\nSELECT 1",
        "--sql\\r\\nSELECT 1",
        "-- sql\\nSELECT 1",
        "-- sql\nSELECT 1",
    ] {
        let source = format!("q = SQL[int](\"\"\"{managed}\"\"\")");
        let stripped = strip_source(&source, None);
        assert_eq!(stripped, format!("q = \"\"\"{managed}\"\"\""), "{managed}");
    }
    for unmanaged in ["-- sql query for users", "SELECT 1", "custom query"] {
        let source = format!("q = SQL[int](\"{unmanaged}\")");
        assert_eq!(strip_source(&source, None), source, "{unmanaged}");
    }
}

#[test]
fn test_rewrite_with_alias() {
    let result = rewrite_source(
        r#"q = "--sql\nSELECT 1::INTEGER""#,
        "postgres",
        None,
        Some("from sqlty import SQL as Q"),
        false,
    );
    assert!(result.contains("Q[tuple[int]]"));
    assert!(result.contains("from sqlty import SQL as Q"));
    assert!(!result.contains("from sqlty import SQL\n"));
}

#[test]
fn test_rewrite_with_module_import() {
    let result = rewrite_source(
        r#"q = "--sql\nSELECT 1::INTEGER""#,
        "postgres",
        None,
        Some("import sqlty"),
        false,
    );
    assert!(result.contains("sqlty.SQL[tuple[int]]"));
    assert!(result.contains("import sqlty"));
}

#[test]
fn test_rewrite_with_module_alias() {
    let result = rewrite_source(
        r#"q = "--sql\nSELECT 1::INTEGER""#,
        "postgres",
        None,
        Some("import sqlty as sy"),
        false,
    );
    assert!(result.contains("sy.SQL[tuple[int]]"));
    assert!(result.contains("import sqlty as sy"));
}

#[test]
fn test_strip_with_alias_roundtrip() {
    let original = r#"q = "--sql\nSELECT 1::INTEGER""#;
    let style = Some("from sqlty import SQL as Q");
    let rewritten = rewrite_source(original, "postgres", None, style, false);
    let stripped = strip_source(&rewritten, style);
    assert_eq!(stripped.trim(), original.trim());
}

#[test]
fn test_strip_with_module_import_roundtrip() {
    let original = r#"q = "--sql\nSELECT 1::INTEGER""#;
    let style = Some("import sqlty");
    let rewritten = rewrite_source(original, "postgres", None, style, false);
    let stripped = strip_source(&rewritten, style);
    assert_eq!(stripped.trim(), original.trim());
}

#[test]
fn test_strip_with_module_alias_roundtrip() {
    let original = r#"q = "--sql\nSELECT 1::INTEGER""#;
    let style = Some("import sqlty as sy");
    let rewritten = rewrite_source(original, "postgres", None, style, false);
    let stripped = strip_source(&rewritten, style);
    assert_eq!(stripped.trim(), original.trim());
}

#[test]
fn test_untyped_skipped_by_default() {
    let source = r#"q = "--sql\nSELECT x, y FROM pairs""#;
    assert_eq!(
        rewrite_source(source, "postgres", None, None, false),
        source
    );
}

#[test]
fn test_untyped_sentinel() {
    let source = r#"q = "--sql\nSELECT x, y FROM pairs""#;
    let result = rewrite_source(source, "postgres", None, None, true);
    assert_eq!(
        result,
        "from sqlty import SQL\nfrom sqlty import UNTYPED\nq = SQL[UNTYPED](\"--sql\\nSELECT x, y FROM pairs\")"
    );
    assert_eq!(strip_source(&result, None), source);
}

#[test]
fn test_untyped_sentinel_module_import() {
    let source = r#"q = "--sql\nSELECT x, y FROM pairs""#;
    let style = Some("import sqlty as sy");
    let result = rewrite_source(source, "postgres", None, style, true);
    assert_eq!(
        result,
        "import sqlty as sy\nq = sy.SQL[sy.UNTYPED](\"--sql\\nSELECT x, y FROM pairs\")"
    );
    assert_eq!(strip_source(&result, style), source);
}

#[test]
fn test_untyped_skips_statements_without_rows() {
    for source in [
        r#"q = "--sql\nDELETE FROM users""#,
        r#"q = "--sql\nINSERT INTO users (id) VALUES (1)""#,
        r#"q = "--sql\nCREATE TABLE users (id INTEGER)""#,
        r#"q = "--sql\nBEGIN""#,
        r#"q = "--sql\n""#,
        r#"q = "--sql SELECT x, y FROM pairs""#,
    ] {
        assert_eq!(rewrite_source(source, "postgres", None, None, true), source);
    }
}

#[test]
fn test_untyped_wraps_unparseable() {
    let source = r#"q = "--sql\nSELECT id FROM""#;
    let result = rewrite_source(source, "postgres", None, None, true);
    assert!(result.ends_with(r#"q = SQL[UNTYPED]("--sql\nSELECT id FROM")"#));
}

#[test]
fn test_multiline_triple_quoted_roundtrip() {
    let source = "q = \"\"\"--sql\nSELECT id::INTEGER\nFROM users\"\"\"\n";
    let result = rewrite_source(source, "postgres", None, None, false);
    assert_eq!(
        result,
        "from sqlty import SQL\nq = SQL[tuple[int]](\"\"\"--sql\nSELECT id::INTEGER\nFROM users\"\"\")\n"
    );
    assert_eq!(strip_source(&result, None), source);
}

#[test]
fn test_prefixed_strings_skipped() {
    let source = "a = f\"--sql\\nSELECT {x}::INTEGER\"\nb = r\"--sql\\nSELECT 1::INTEGER\"";
    assert_eq!(
        rewrite_source(source, "postgres", None, None, false),
        source
    );
}

fn wrapped(content: &str) -> bool {
    let source = format!("q = \"\"\"{content}\"\"\"\n");
    rewrite_source(&source, "postgres", None, None, false).contains("SQL[tuple[int]](")
}

#[test]
fn test_markers_infer() {
    for content in [
        r"--sql\nSELECT 1",
        r"--sql\r\nSELECT 1",
        r"-- sql\nSELECT 1",
        "--sql\nSELECT 1",
        "-- sql\r\nSELECT 1",
    ] {
        assert!(wrapped(content), "{content}");
    }
    for content in ["--sql SELECT 1", "-- sql SELECT 1", r"-- sqlx\nSELECT 1"] {
        assert!(!wrapped(content), "{content}");
    }
}

#[test]
fn test_ends_marker_line() {
    // The `--sql` marker must end its line, or MySQL would read the rest as part of it.
    let options = Options::new("mysql", None, None, false).unwrap();
    let marked = |rest: &str| {
        let source = format!("q = \"\"\"--sql{rest}\"\"\"\n");
        sqlty_python::checker::check_source(&source, &options, &RuleConfig::default(), &[])
            .iter()
            .any(|d| d.rule == Rule::MysqlSqlMarker)
    };
    for rest in [
        "\nSELECT 1",
        r"\nSELECT 1",
        r"\r\nSELECT 1",
        "  \nSELECT 1",
        " -- users\nSELECT 1",
        " sqlty:db=analytics\nSELECT 1",
    ] {
        assert!(marked(rest), "{rest:?}");
    }
    for rest in [
        "SELECT 1",
        " query notes\nSELECT 1",
        "-- users\nSELECT 1",
        " SELECT 1",
    ] {
        assert!(!marked(rest), "{rest:?}");
    }
}

fn keep_lines(source: &str) -> String {
    let options = Options::new("postgres", None, None, false).unwrap();
    rewrite_source_with(source, &options, true)
}

const Q: &str = "q = \"--sql\\nSELECT 1::INTEGER AS x\"\n";

#[test]
fn test_keep_lines_script_mode_docstring() {
    let source = format!("\n\"\"\"Doc.\"\"\"  # c\n{Q}");
    assert_eq!(
        keep_lines(&source),
        "\n\"\"\"Doc.\"\"\"; from sqlty import SQL  # c\nq = SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER AS x\")\n"
    );
}

#[test]
fn test_keep_lines_shebang_cookie_simple_statement() {
    let source = format!("#!/usr/bin/env python\n# coding: sqlty\n{Q}");
    let result = keep_lines(&source);
    assert!(result
        .starts_with("#!/usr/bin/env python\n# coding: sqlty\nfrom sqlty import SQL; q = SQL["));
    assert!(keep_lines(&format!("\n{Q}")).starts_with("\nfrom sqlty import SQL; q = SQL["));
}

#[test]
fn test_keep_lines_future() {
    let source = format!("\nfrom __future__ import (\n    annotations,\n)  # c\n{Q}");
    assert!(keep_lines(&source).starts_with(
        "\nfrom __future__ import (\n    annotations,\n); from sqlty import SQL  # c\n"
    ));
}

#[test]
fn test_keep_lines_compound_statement() {
    let def = format!("\ndef f():\n    pass\n{Q}");
    assert!(keep_lines(&def).starts_with("\nfrom sqlty import SQL\ndef f():\n"));
    let blank = format!("\n\n@decorator\ndef f():\n    pass\n{Q}");
    assert!(keep_lines(&blank).starts_with("\nfrom sqlty import SQL\n@decorator\n"));
    let comment = format!("\n# c\nif x: y\n{Q}");
    assert!(keep_lines(&comment).starts_with("\nfrom sqlty import SQL  # c\nif x: y\n"));
    let doc = format!("\n\"\"\"Doc.\"\"\"\nclass A: pass\n{Q}");
    assert!(keep_lines(&doc).starts_with("\n\"\"\"Doc.\"\"\"; from sqlty import SQL\nclass A"));
}

#[test]
fn test_keep_lines_docstring_semicolon_and_existing_import() {
    let doc = format!("\n\"\"\"Doc.\"\"\"; x = 1\n{Q}");
    assert!(keep_lines(&doc).starts_with("\n\"\"\"Doc.\"\"\"; from sqlty import SQL; x = 1\n"));
    let existing = format!("\nfrom sqlty import SQL\n{Q}");
    assert_eq!(
        keep_lines(&existing),
        rewrite_source(&existing, "postgres", None, None, false)
    );
}

#[test]
fn test_keep_lines_placement() {
    let imp = "from sqlty import SQL";
    for (header, expected) in [
        ("# coding: sqlty\n", format!("# coding: sqlty\n{imp}; q = ")),
        (
            "\"\"\"Multi\nline.\n\"\"\"\n",
            format!("\"\"\"Multi\nline.\n\"\"\"; {imp}\nq = "),
        ),
        (
            "\n\"\"\"Doc.\"\"\"\nfrom __future__ import annotations\n",
            format!("\n\"\"\"Doc.\"\"\"\nfrom __future__ import annotations; {imp}\nq = "),
        ),
        (
            "\n  # c\nclass A: pass\n",
            format!("\n{imp}    # c\nclass A: pass\nq = "),
        ),
        (
            "\nmatch x:\n    case _: pass\n",
            format!("\n{imp}\nmatch x:\n    case _: pass\nq = "),
        ),
        ("\nmatch = 1\n", format!("\n{imp}; match = 1\nq = ")),
    ] {
        let result = keep_lines(&format!("{header}{Q}"));
        assert!(result.starts_with(&expected), "{result:?}");
    }
    let continued = keep_lines("\nq = \\\n    \"--sql\\nSELECT 1::INTEGER AS x\"\n");
    assert!(
        continued.starts_with(&format!("\n{imp}; q = \\\n    SQL[")),
        "{continued:?}"
    );
}

#[test]
fn test_keep_lines_datetime_uuid() {
    let source =
        "\n\"\"\"Doc.\"\"\"\r\nq = \"--sql\\nSELECT NOW()::TIMESTAMP AS t, 'a'::UUID AS u\"\r\n";
    assert!(keep_lines(source).starts_with(
        "\n\"\"\"Doc.\"\"\"; import datetime; import uuid; from sqlty import SQL\r\nq = SQL[tuple[datetime.datetime, uuid.UUID]]("
    ));
}

#[test]
fn test_keep_lines_preserves_line_count() {
    for source in [
        format!("\n\"\"\"Doc.\"\"\"\n{Q}"),
        format!("\nfrom __future__ import annotations\n{Q}"),
        format!("\n\n@decorator\ndef f():\n    pass\n{Q}"),
        format!("\nimport os\n{Q}"),
        format!("\n\"\"\"Doc.\"\"\"\r\n{Q}").replace('\n', "\r\n"),
    ] {
        let result = keep_lines(&source);
        assert!(result.contains("SQL[tuple[int]]"), "{result}");
        assert_eq!(result.lines().count(), source.lines().count(), "{result}");
        assert_eq!(result.lines().next(), source.lines().next(), "{result}");
    }
}

#[test]
fn a_module_without_queries_is_unchanged() {
    assert_eq!(rewrite_source("x", "postgres", None, None, false), "x");
}
