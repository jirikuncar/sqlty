use crate::common::*;
use sqlty_python::checker::query_sites;

fn check_source(
    source: &str,
    dialect: &str,
    schema: Option<&SchemaData>,
    rules: &RuleConfig,
    style: Option<&str>,
    untyped: bool,
    any_calls: &[usize],
) -> Vec<Diagnostic> {
    let options = Options {
        dialect: dialect.parse().unwrap(),
        schema,
        style: style_or_default(style),
        untyped,
    };
    sqlty_python::checker::check_source(source, &options, rules, any_calls)
}

fn override_rule(rule: &str, severity: &str) -> RuleConfig {
    let overrides = HashMap::from([(rule.to_string(), severity.to_string())]);
    RuleConfig::from_overrides(Some(overrides)).unwrap()
}

#[test]
fn test_detects_stale_rewrite() {
    let source = r#"q = "--sql\nSELECT 1::INTEGER""#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule, Rule::StaleRewrite);
}

#[test]
fn test_stale_rewrite_fix_imports_the_wrapper() {
    let options = Options {
        dialect: "postgres".parse().unwrap(),
        schema: None,
        style: style_or_default(None),
        untyped: false,
    };
    let diags = sqlty_python::checker::check_source_with_imports(
        r#"q = "--sql\nSELECT 1::INTEGER""#,
        &options,
        &RuleConfig::default(),
        &[],
    );
    let [diag] = diags.as_slice() else {
        panic!("{diags:?}")
    };
    assert_eq!(diag.rule, Rule::StaleRewrite);
    assert!(
        diag.fix
            .iter()
            .any(|edit| edit.replacement.contains("from sqlty import SQL")),
        "{:?}",
        diag.fix
    );
}

#[test]
fn test_stale_rewrite_agrees_with_rewrite() {
    for (source, line) in [
        ("q = \"\"\"\n--sql\nSELECT 1::INTEGER AS n\"\"\"", 1),
        (
            "x = 1\nq = SQL[tuple[int]](\"--sql\\nSELECT 1\"), \"--sql\\nSELECT 2\"",
            2,
        ),
        ("q = '  --sql\\nSELECT 1'", 1),
    ] {
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        let lines: Vec<usize> = diags
            .iter()
            .filter(|d| d.rule == Rule::StaleRewrite)
            .map(|d| d.line)
            .collect();
        assert_eq!(lines, vec![line], "{source}");
    }
}

#[test]
fn test_unwrappable_string_is_not_stale() {
    for source in [
        r#"q = "--sql SELECT 1::INTEGER""#,
        r#"q = "--sql\nDROP TABLE t""#,
    ] {
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        assert!(
            diags.iter().all(|d| d.rule != Rule::StaleRewrite),
            "{source}"
        );
    }
}

#[test]
fn test_untyped_rows_are_stale_with_sentinel() {
    let source = r#"q = "--sql\nSELECT x, y FROM pairs""#;
    assert!(check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[]
    )
    .is_empty());
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        true,
        &[],
    );
    assert_eq!(diags[0].rule, Rule::StaleRewrite);
}

#[test]
fn test_clean_no_stale() {
    let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    );
    let stale_rewrites: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::StaleRewrite)
        .collect();
    assert!(stale_rewrites.is_empty());
}

#[test]
fn test_detects_stale_type() {
    let source = r#"q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM t")"#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    );
    let narrow: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::NarrowType)
        .collect();
    assert_eq!(narrow.len(), 1);
    assert!(narrow[0].message.contains("tuple[int]"));
}

#[test]
fn test_rule_override_ignore() {
    let source = r#"q = "--sql SELECT 1::INTEGER""#;
    let rules = override_rule("stale-rewrite", "ignore");
    let diags = check_source(source, "postgres", None, &rules, None, false, &[]);
    let stale_rewrites: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::StaleRewrite)
        .collect();
    assert!(stale_rewrites.is_empty());
}

#[test]
fn test_detects_inline_sql_comment_unwrapped() {
    let source = r#"q = "--sql SELECT 1::INTEGER""#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    );
    let inline: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::InlineSqlComment)
        .collect();
    assert_eq!(inline.len(), 1);
    assert!(inline[0].message.contains("SQL comment"));
}

#[test]
fn test_no_inline_sql_comment_with_newline() {
    let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    );
    let inline: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::InlineSqlComment)
        .collect();
    assert!(inline.is_empty());
}

#[test]
fn test_detects_inline_sql_comment_wrapped() {
    let source = r#"q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")"#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    );
    let inline: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::InlineSqlComment)
        .collect();
    assert_eq!(inline.len(), 1);
}

#[test]
fn test_inline_sql_comment_ignored_when_disabled() {
    let source = r#"q = "--sql SELECT 1::INTEGER""#;
    let rules = override_rule("inline-sql-comment", "ignore");
    let diags = check_source(source, "postgres", None, &rules, None, false, &[]);
    let inline: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::InlineSqlComment)
        .collect();
    assert!(inline.is_empty());
}

#[test]
fn test_check_with_alias_import_style() {
    let source = r#"q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        Some("from sqlty import SQL as Q"),
        false,
        &[],
    );
    let stale_rewrites: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::StaleRewrite)
        .collect();
    assert!(stale_rewrites.is_empty());
}

#[test]
fn test_check_with_module_import_style() {
    let source = r#"q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        Some("import sqlty"),
        false,
        &[],
    );
    let stale_rewrites: Vec<_> = diags
        .iter()
        .filter(|d| d.rule == Rule::StaleRewrite)
        .collect();
    assert!(stale_rewrites.is_empty());
}

#[test]
fn test_name_rules_are_on_by_default() {
    let schema = Some(sqlty_sql::analyzer::load_schema_with_skipped(
        "CREATE TABLE users (id INT)",
        "postgres",
    ));
    let schema = schema.map(|(schema, _)| schema);
    let source = r#"q = SQL[tuple[int]]("--sql\nSELECT nope FROM users")"#;
    assert!(!check_source(
        source,
        "postgres",
        schema.as_deref(),
        &RuleConfig::default(),
        None,
        false,
        &[]
    )
    .is_empty());
    let rules = override_rule("unknown-column", "error");
    let diags = check_source(
        source,
        "postgres",
        schema.as_deref(),
        &rules,
        None,
        false,
        &[],
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule, Rule::UnknownColumn);
    assert_eq!(diags[0].severity, Severity::Error);
}

#[test]
fn test_untyped_query() {
    let source = r#"q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")"#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule, Rule::UntypedQuery);
    assert!(diags[0].message.contains("add casts"));
}

#[test]
fn test_untyped_query_now_inferable() {
    let source = r#"q = sqlty.SQL[sqlty.UNTYPED]("--sql\nSELECT 1::INTEGER")"#;
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        Some("import sqlty"),
        false,
        &[],
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].message,
        "sqlty.SQL[sqlty.UNTYPED] can now be sqlty.SQL[tuple[int]]; run sqlty check --fix"
    );
}

#[test]
fn test_any_calls_skip_unchecked_query() {
    let source = "a = SQL[A](\"--sql\\nSELECT 1\" + x)\nb = SQL[A](r\"--sql\\nSELECT 1\")\nc = SQL[A](\"--sql\\nSELECT 1\" + x)\n";
    let any: Vec<usize> = source
        .match_indices("SQL")
        .map(|(i, _)| i)
        .take(2)
        .collect();
    let lines = |diags: Vec<Diagnostic>| diags.into_iter().map(|d| d.line).collect::<Vec<_>>();
    assert_eq!(
        lines(check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &any
        )),
        [3]
    );
    assert_eq!(
        lines(check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[]
        )),
        [1, 2, 3]
    );
    let sites = query_sites(source, &ImportStyle::default(), &any);
    assert_eq!(sites.iter().map(|s| s.0).collect::<Vec<_>>(), [3]);
}

#[test]
fn test_skipped_and_unchecked_queries() {
    let source = "a = (\"--sql\\nSELECT 1 \"\n     f\"AS n{x}\")\nb = SQL[T](\"--sql\\nSELECT 1\" + x)\nc = r\"--sql\\nSELECT 1\"\n";
    let diags: Vec<(Rule, usize)> = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    )
    .into_iter()
    .map(|d| (d.rule, d.line))
    .collect();
    assert_eq!(
        diags,
        vec![
            (Rule::FstringSkipped, 1),
            (Rule::UncheckedQuery, 3),
            (Rule::UncheckedQuery, 4),
        ]
    );
}

#[test]
fn test_every_wrapper_on_a_line() {
    let source = "a, b = SQL[tuple[int]](\"--sql\\nSELECT 1\"), SQL[tuple[int]](\"--sql\\nSELECT 'x'::TEXT\")";
    let diags = check_source(
        source,
        "postgres",
        None,
        &RuleConfig::default(),
        None,
        false,
        &[],
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].message,
        "SQL[tuple[int]] should be SQL[tuple[str]]"
    );
}

#[test]
fn rarer_untyped_causes_unwrapped_calls_and_unexpanded_stars() {
    let (schema, _) = sqlty_sql::analyzer::load_schema_with_skipped(
        r#"CREATE TABLE t ("Id" INT, a INT);"#,
        "postgres",
    );
    let check = |source: &str| {
        check_source(
            source,
            "postgres",
            Some(&schema),
            &RuleConfig::default(),
            None,
            false,
            &[],
        )
    };
    let found: Vec<String> = check(concat!(
        "q = SQL[UNTYPED](\"--sql\\nSELECT 1::nope\")\n",
        "r = SQL[UNTYPED](\"--sql\\nSELECT nope FROM t\")\n",
    ))
    .into_iter()
    .filter(|d| d.rule == Rule::UntypedQuery)
    .map(|d| d.message)
    .collect();
    assert!(
        found[0].ends_with("; declare the type in the schema"),
        "{found:?}"
    );
    assert!(!found[1].ends_with("in the schema"), "{found:?}");
    assert!(check("from sqlty import SQL\nq = SQL(\"SELEC 1\")\n").is_empty());
    let star = check("q = SQL[tuple[int, int]](\"--sql\\nSELECT * FROM t\")\n");
    assert_eq!(star[0].rule, Rule::SelectStar);
    assert!(star[0].fix.is_empty());
}
