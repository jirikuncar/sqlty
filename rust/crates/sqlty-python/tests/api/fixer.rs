use crate::common::*;
use sqlty_python::fixer::*;
use sqlty_python::rewriter::Suppressed;

fn fix(source: &str) -> String {
    let options = Options::new("postgres", None, None, false).unwrap();
    fix_source(source, &options, &RuleConfig::default(), &Suppressed::new())
}

#[test]
fn test_stale_type_updated() {
    assert_eq!(
        fix("from sqlty import SQL\nq = SQL[tuple[str]](\"--sql\\nSELECT 1::INTEGER\")"),
        "from sqlty import SQL\nq = SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER\")"
    );
}

#[test]
fn test_wrapper_argument_spans_to_closing_paren() {
    assert_eq!(
        fix("q = SQL[tuple[str]](\"--sql\\nSELECT 1::INTEGER \"\n  \"AS n\")"),
        "q = SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER \"\n  \"AS n\")"
    );
    assert_eq!(
        fix("q = SQL[tuple[str]](\"\"\"--sql\nSELECT 1::INTEGER AS n\"\"\"  # c\n)"),
        "q = SQL[tuple[int]](\"\"\"--sql\nSELECT 1::INTEGER AS n\"\"\"  # c\n)"
    );
}

#[test]
fn test_inline_comment_only_in_plain_strings() {
    assert_eq!(
        fix(r#"a = "--sql SELECT 1::INTEGER"
b = f"--sql SELECT 1""#),
        r#"from sqlty import SQL
a = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
b = f"--sql SELECT 1""#
    );
}

#[test]
fn test_user_types_untouched() {
    let source = "from sqlty import SQL\nq = SQL[User](\"--sql\\nSELECT 1::INTEGER\")";
    assert_eq!(fix(source), source);
}

#[test]
fn test_datetime_import_added() {
    assert_eq!(
        fix("from sqlty import SQL\nq = SQL[tuple[str]](\"--sql\\nSELECT now()\")"),
        "import datetime\nfrom sqlty import SQL\nq = SQL[tuple[datetime.datetime]](\"--sql\\nSELECT now()\")"
    );
}

#[test]
fn test_datetime_import_uses_crlf() {
    assert_eq!(
        fix("from sqlty import SQL\r\nq = SQL[tuple[str]](\"--sql\\nSELECT now()\")\r\n"),
        "import datetime\r\nfrom sqlty import SQL\r\nq = SQL[tuple[datetime.datetime]](\"--sql\\nSELECT now()\")\r\n"
    );
}

#[test]
fn test_mysql_markers_fixed_in_plain_literals() {
    let source = r#"a = "--sql\nSELECT 1"
b = """  --sql
SELECT 1"""
c = f"--sql\nSELECT 1"
d = "--sql SELECT 1""#;
    let fixed = r#"a = "-- sql\nSELECT 1"
b = """  -- sql
SELECT 1"""
c = f"--sql\nSELECT 1"
d = "--sql SELECT 1""#;
    let options = Options::new("mysql", None, None, false).unwrap();
    let rules = RuleConfig::from_overrides(Some(
        [
            ("stale-rewrite".to_string(), "ignore".to_string()),
            ("inline-sql-comment".to_string(), "ignore".to_string()),
        ]
        .into(),
    ))
    .unwrap();
    assert_eq!(
        fix_source(source, &options, &rules, &Suppressed::new()),
        fixed
    );
}
