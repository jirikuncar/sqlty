use crate::common::*;
use sqlty_python::rows::Reported;
use sqlty_python::suppress::*;
use sqlty_sql::analyzer::statement_ranges;

#[test]
fn a_named_ignore_suppresses_its_rule_without_unused_ignore() {
    let source = Source::new("q = 1  # sqlty: ignore[select-star]\n", (3, 14)).unwrap();
    let reported = Reported {
        diagnostic: Diagnostic {
            rule: Rule::SelectStar,
            message: String::new(),
            severity: Severity::Error,
            line: 1,
            span: 0..1,
            fix: Vec::new(),
        },
        redundant: None,
    };
    let rules = only(&[Rule::SelectStar]);
    assert!(apply(&ignores(&source), vec![reported], &rules).is_empty());
}

#[test]
fn a_crashed_module_reports_no_unused_ignore() {
    let source = Source::new("q = 1  # sqlty: ignore[select-star]\n", (3, 14)).unwrap();
    let reported = Reported {
        diagnostic: Diagnostic {
            rule: Rule::InternalError,
            message: String::new(),
            severity: Severity::Error,
            line: 1,
            span: 0..0,
            fix: Vec::new(),
        },
        redundant: None,
    };
    let rules = only(&[Rule::InternalError, Rule::UnusedIgnore]);
    let kept = apply(&ignores(&source), vec![reported], &rules);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].diagnostic.rule, Rule::InternalError);
}

fn lines_and_rules(found: &[Ignore]) -> Vec<(usize, Option<Vec<&str>>)> {
    found
        .iter()
        .map(|ignore| {
            let rules = ignore
                .rules
                .as_ref()
                .map(|rules| rules.iter().map(String::as_str).collect());
            (ignore.line, rules)
        })
        .collect()
}

#[test]
fn a_source_that_does_not_parse_keeps_every_comment_the_lexer_finds() {
    let text = "# note\nx = (  # sqlty: ignore\ny = $  # sqlty: ignore[a]\nz = '# sqlty: ignore\n";
    assert!(Source::new(text, (3, 14)).is_none());
    let found = unparsed_ignores(text);
    assert_eq!(lines_and_rules(&found), [(2, None), (3, Some(vec!["a"]))]);
    assert_eq!(&text[found[0].span.0..found[0].span.1], "# sqlty: ignore");
    assert!(unparsed_ignores("x = (\n").is_empty());
}

#[test]
fn sql_comments_suppress_on_their_line() {
    let text =
        "-- name: a\nSELECT 1; -- sqlty: ignore[x, y ]\r\n--sqlty:ignore\r-- sqlty: ignored\nSELECT 2;\n";
    let found = sql_ignores(text, &[]);
    assert_eq!(
        lines_and_rules(&found),
        [(2, Some(vec!["x", "y"])), (3, None)]
    );
    assert_eq!(&text[found[1].span.0..found[1].span.1], "--sqlty:ignore");
    assert!(sql_ignores("SELECT 1", &[]).is_empty());

    let diagnostic = |rule, line| Diagnostic {
        rule,
        message: String::new(),
        severity: Severity::Error,
        line,
        span: 0..1,
        fix: Vec::new(),
    };
    let rules = only(&[Rule::SelectStar, Rule::UnusedIgnore]);
    let kept = apply_sql(&found, vec![diagnostic(Rule::SelectStar, 4)], &rules);
    let messages: Vec<_> = kept.iter().map(|found| found.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "'-- sqlty: ignore[x]' suppresses no diagnostic",
            "'-- sqlty: ignore[y]' suppresses no diagnostic",
        ]
    );
}

fn covers(text: &str) -> Vec<(usize, usize)> {
    sql_ignores(text, &[])
        .iter()
        .map(|ignore| (ignore.line, ignore.covers))
        .collect()
}

#[test]
fn a_sql_comment_alone_on_its_line_covers_the_next_statement() {
    assert_eq!(covers("-- sqlty: ignore\nSELECT 1;\n"), [(1, 2)]);
    assert_eq!(
        covers("-- sqlty: ignore[a]\n-- sqlty: ignore[b]\nSELECT 1;\n"),
        [(1, 3), (2, 3)]
    );
    assert_eq!(
        covers("-- sqlty: ignore\n-- name: q\n-- error: x\nSELECT 1;"),
        [(1, 4)]
    );
    assert_eq!(covers("-- sqlty: ignore\n\nSELECT 1;\n"), [(1, 1)]);
    assert_eq!(covers("-- sqlty: ignore\n  \t\nSELECT 1;\n"), [(1, 1)]);
    assert_eq!(covers("SELECT 1;\n-- sqlty: ignore\n"), [(2, 2)]);
    assert_eq!(covers("SELECT 1;\n-- sqlty: ignore\n-- note"), [(2, 2)]);
    assert_eq!(covers("-- sqlty: ignore\r\nSELECT 1;\r\n"), [(1, 2)]);
    assert_eq!(covers("\t-- sqlty: ignore\n\tSELECT 1;\n"), [(1, 2)]);
    assert_eq!(
        covers("SELECT 'é\u{1F600}'; -- sqlty: ignore\nSELECT 1;"),
        [(1, 1)]
    );
    assert_eq!(covers("  -- sqlty: ignore\r\n\tSELECT 1;"), [(1, 2)]);
    assert_eq!(covers("SELECT 1; -- sqlty: ignore\nSELECT 2;"), [(1, 1)]);
    assert_eq!(
        covers("-- sqlty: ignore[a]\nSELECT 1; -- sqlty: ignore[b]\n"),
        [(1, 2), (2, 2)]
    );
    assert_eq!(
        covers("-- sqlty: ignore\nSELECT 1;\n-- sqlty: ignore\n\nSELECT 2;"),
        [(1, 2), (3, 3)]
    );
    // Diagnostics, the CLI and the language server end lines at `\n` only.
    assert_eq!(covers("-- sqlty: ignore\rSELECT 1;\nSELECT 2;"), [(1, 2)]);
    assert_eq!(covers("SELECT 1;\r-- sqlty: ignore\nSELECT 2;"), [(1, 1)]);
    assert_eq!(
        covers("SELECT 1;\r\r-- sqlty: ignore[a]\nSELECT 2;"),
        [(1, 1)]
    );
    let many = "-- sqlty: ignore\n".repeat(1000) + "SELECT 1;";
    assert!(covers(&many).iter().all(|&(_, covered)| covered == 1001));
}

fn ranges(text: &str) -> Vec<(usize, usize, usize)> {
    let statements = statement_ranges(text, "postgres");
    sql_ignores(text, &statements)
        .iter()
        .map(|ignore| (ignore.line, ignore.covers, ignore.through))
        .collect()
}

#[test]
fn a_sql_comment_alone_on_its_line_covers_the_statement_below_it() {
    assert_eq!(
        ranges("-- sqlty: ignore\nSELECT id\nFROM nope\n;\nSELECT 2;"),
        [(1, 2, 3)]
    );
    assert_eq!(
        ranges("-- sqlty: ignore\n/* why */\n  /**/\nSELECT id\nFROM nope;"),
        [(1, 4, 5)]
    );
    // Not wholly a comment: the statement starts on that line.
    assert_eq!(ranges("-- sqlty: ignore\n/* a */ SELECT\n1;"), [(1, 2, 3)]);
    assert_eq!(
        ranges("-- sqlty: ignore\n/* a */ */\nSELECT 1;"),
        [(1, 2, 3)]
    );
    assert_eq!(ranges("-- sqlty: ignore\n/*/\nSELECT 1;"), [(1, 2, 2)]);
    // Two statements start on the line: the cover runs to the end of the longer.
    assert_eq!(
        ranges("-- sqlty: ignore\nSELECT 1; SELECT\n2;\nSELECT 3;"),
        [(1, 2, 3)]
    );
    // Inside a statement, or on a statement's line, a comment covers one line.
    assert_eq!(
        ranges("SELECT a,\n-- sqlty: ignore\nb\nFROM t;"),
        [(2, 3, 3)]
    );
    assert_eq!(ranges("SELECT a -- sqlty: ignore\nFROM t;"), [(1, 1, 1)]);
    let text = "-- sqlty: ignore[unknown-table]\nSELECT id\nFROM nope;\n";
    let diagnostic = Diagnostic {
        rule: Rule::UnknownTable,
        message: String::new(),
        severity: Severity::Error,
        line: 3,
        span: 22..26,
        fix: Vec::new(),
    };
    let statements = statement_ranges(text, "postgres");
    let rules = only(&[Rule::UnknownTable, Rule::UnusedIgnore]);
    let kept = apply_sql(&sql_ignores(text, &statements), vec![diagnostic], &rules);
    assert!(kept.is_empty(), "{kept:?}");
}

#[test]
fn a_second_comment_for_one_statement_is_unused() {
    let text = "-- sqlty: ignore[select-star]\nSELECT * FROM t; -- sqlty: ignore[select-star]\n";
    let diagnostic = Diagnostic {
        rule: Rule::SelectStar,
        message: String::new(),
        severity: Severity::Error,
        line: 2,
        span: 0..1,
        fix: Vec::new(),
    };
    let rules = only(&[Rule::SelectStar, Rule::UnusedIgnore]);
    let kept = apply_sql(&sql_ignores(text, &[]), vec![diagnostic], &rules);
    let [unused] = kept.as_slice() else {
        panic!("{kept:?}");
    };
    assert_eq!((unused.rule, unused.line), (Rule::UnusedIgnore, 2));
}

#[test]
fn fixes_skip_suppressed_lines_and_the_strings_of_helper_calls() {
    let ignores = [
        Ignore {
            line: 1,
            covers: 1,
            through: 1,
            rules: None,
            span: (0, 1),
        },
        Ignore {
            line: 2,
            covers: 2,
            through: 2,
            rules: Some(vec!["select-star".to_string()]),
            span: (0, 1),
        },
    ];
    let found = suppressed(&ignores, [(1, 3)]);
    assert_eq!(found[&1], None);
    assert_eq!(
        found[&2],
        Some(vec!["select-star".to_string(), "stale-rewrite".to_string()])
    );
    assert_eq!(found[&3], Some(vec!["stale-rewrite".to_string()]));
}
