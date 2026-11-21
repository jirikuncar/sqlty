use crate::common::*;
use sqlty_python::rows::Reported;
use sqlty_python::suppress::*;

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
        "-- name: a\nSELECT 1; -- sqlty: ignore[x, y ]\r\n--sqlty:ignore\r-- sqlty: ignored\n";
    let found = sql_ignores(text);
    assert_eq!(
        lines_and_rules(&found),
        [(2, Some(vec!["x", "y"])), (3, None)]
    );
    assert_eq!(&text[found[1].span.0..found[1].span.1], "--sqlty:ignore");
    assert!(sql_ignores("SELECT 1").is_empty());

    let diagnostic = |rule, line| Diagnostic {
        rule,
        message: String::new(),
        severity: Severity::Error,
        line,
        span: 0..1,
        fix: Vec::new(),
    };
    let rules = only(&[Rule::SelectStar, Rule::UnusedIgnore]);
    let kept = apply_sql(&found, vec![diagnostic(Rule::SelectStar, 3)], &rules);
    let messages: Vec<_> = kept.iter().map(|found| found.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "'-- sqlty: ignore[x]' suppresses no diagnostic",
            "'-- sqlty: ignore[y]' suppresses no diagnostic",
        ]
    );
}

#[test]
fn fixes_skip_suppressed_lines_and_the_strings_of_helper_calls() {
    let ignores = [
        Ignore {
            line: 1,
            rules: None,
            span: (0, 1),
        },
        Ignore {
            line: 2,
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
