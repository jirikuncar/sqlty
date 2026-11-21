//! The migration checks of `sqlty breaking`, with the user's severities and suppressions.

use crate::checker::Diagnostic;
use crate::rewriter::LineIndex;
use crate::rules::{Rule, RuleConfig};
use crate::suppress::{apply_sql, sql_ignores};

/// The diagnostics of each of `files`, PostgreSQL migrations in the order they run, for the
/// statements `baseline` does not have. `-- sqlty: ignore[rule]` on a statement's first line,
/// or alone on its line above the statement, drops its diagnostic.
pub fn check_migrations(
    baseline: &str,
    files: &[&str],
    rules: &RuleConfig,
) -> Vec<Vec<Diagnostic>> {
    let files: Vec<(&str, &RuleConfig)> = files.iter().map(|sql| (*sql, rules)).collect();
    check_migrations_with(baseline, &files)
}

/// [`check_migrations`] with the rules of each file: `[(sql, rules)]`, the severities of
/// the overrides its path matches.
pub fn check_migrations_with(
    baseline: &str,
    files: &[(&str, &RuleConfig)],
) -> Vec<Vec<Diagnostic>> {
    let sources: Vec<&str> = files.iter().map(|(sql, _)| *sql).collect();
    sqlty_sql::migrations::check(baseline, &sources)
        .into_iter()
        .zip(files)
        .map(|(found, (sql, rules))| {
            let lines = LineIndex::new(sql);
            let found: Vec<Diagnostic> = found
                .into_iter()
                .filter(|finding| rules.is_enabled(finding.rule))
                .map(|finding| Diagnostic {
                    rule: finding.rule,
                    message: finding.message,
                    severity: rules.severity(finding.rule),
                    line: lines.line_at(finding.span.start),
                    span: finding.span,
                    fix: Vec::new(),
                })
                .collect();
            // Each finding spans its statement.
            let statements: Vec<_> = found.iter().map(|found| found.span.clone()).collect();
            apply_sql(&sql_ignores(sql, &statements), found, rules)
                .into_iter()
                .filter(|diagnostic| diagnostic.rule != Rule::UnusedIgnore)
                .collect()
        })
        .collect()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::rules::Severity;

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
        let above = "SET lock_timeout = '1s';\n-- sqlty: ignore[missing-concurrently]\nCREATE INDEX i ON users (bio);";
        let found = check_migrations(BASELINE, &[above], &rules(&[]));
        assert!(found[0].is_empty(), "{found:?}");
        let apart = above.replace("]\nCREATE", "]\n\nCREATE");
        let found = check_migrations(BASELINE, &[&apart], &rules(&[]));
        assert_eq!(found[0].len(), 1, "{found:?}");
        // A lone `\r` ends no line: the comment shares the first line, and covers it alone.
        let cr = "SET lock_timeout = '1s';\r-- sqlty: ignore[missing-concurrently]\nSELECT 1;\nCREATE INDEX i ON users (bio);";
        let found = check_migrations(BASELINE, &[cr], &rules(&[]));
        assert_eq!(found[0].len(), 1, "{found:?}");
        assert_eq!(found[0][0].line, 3, "{found:?}");
        let block = "SET lock_timeout = '1s';\n-- sqlty: ignore[missing-concurrently]\n/* why */\nCREATE INDEX i\nON users (bio);";
        let found = check_migrations(BASELINE, &[block], &rules(&[("unused-ignore", "error")]));
        assert!(found[0].is_empty(), "{found:?}");
    }
}
