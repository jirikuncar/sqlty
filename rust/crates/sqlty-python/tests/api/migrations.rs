use crate::common::*;
use sqlty_python::migrations::*;
use sqlty_python::rules::{Rule, RuleConfig, Severity};

const BASELINE: &str = "CREATE TABLE users (id INT, bio TEXT);";

fn rules(overrides: &[(&str, &str)]) -> RuleConfig {
    let overrides: HashMap<String, String> = overrides
        .iter()
        .map(|(rule, severity)| (rule.to_string(), severity.to_string()))
        .collect();
    RuleConfig::from_overrides(Some(overrides)).unwrap()
}

#[test]
fn diagnostics_have_the_configured_severity_and_line() {
    let migration = "SET lock_timeout = '1s';\n\nCREATE INDEX i\n  ON users (bio);";
    let found = check_migrations(
        BASELINE,
        &[migration],
        &rules(&[("missing-concurrently", "warn")]),
    );
    let [diagnostic] = found[0].as_slice() else {
        panic!("{found:?}");
    };
    assert_eq!(diagnostic.rule, Rule::MissingConcurrently);
    assert_eq!(diagnostic.severity, Severity::Warn);
    assert_eq!(diagnostic.line, 3);
    assert_eq!(
        &migration[diagnostic.span.clone()],
        "CREATE INDEX i\n  ON users (bio)"
    );
}

#[test]
fn each_file_has_its_own_severities() {
    let first = "SET lock_timeout = '1s';\nCREATE INDEX a ON users (bio);";
    let second = "SET lock_timeout = '1s';\nCREATE INDEX b ON users (id);";
    let (warn, error) = (
        rules(&[("missing-concurrently", "warn")]),
        rules(&[("missing-concurrently", "error")]),
    );
    let found = check_migrations_with(BASELINE, &[(first, &warn), (second, &error)]);
    let severities: Vec<Vec<Severity>> = found
        .iter()
        .map(|found| found.iter().map(|d| d.severity).collect())
        .collect();
    assert_eq!(severities, [vec![Severity::Warn], vec![Severity::Error]]);
}

#[test]
fn ignored_rules_and_comments_drop_diagnostics() {
    let migration = "CREATE INDEX i ON users (bio);";
    let off = rules(&[
        ("missing-concurrently", "ignore"),
        ("missing-lock-timeout", "ignore"),
    ]);
    assert!(check_migrations(BASELINE, &[migration], &off)[0].is_empty());
    let commented = "SET lock_timeout = '1s';\nCREATE INDEX i ON users (bio); -- sqlty: ignore[missing-concurrently]\n-- sqlty: ignore[table-rewrite]";
    let found = check_migrations(
        BASELINE,
        &[commented],
        &rules(&[("unused-ignore", "error")]),
    );
    assert!(found.len() == 1 && found[0].is_empty(), "{found:?}");
}
