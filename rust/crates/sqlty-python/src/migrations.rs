//! The migration checks of `sqlty breaking`, with the user's severities and suppressions.

use crate::checker::Diagnostic;
use crate::rules::{Rule, RuleConfig};
use crate::suppress::{apply_sql, sql_ignores};

/// The diagnostics of each of `files`, PostgreSQL migrations in the order they run, for the
/// statements `baseline` does not have. `-- sqlty: ignore[rule]` on a statement's first line,
/// or alone on the line above it, drops its diagnostic.
pub fn check_migrations(
    baseline: &str,
    files: &[&str],
    rules: &RuleConfig,
) -> Vec<Vec<Diagnostic>> {
    sqlty_sql::migrations::check(baseline, files)
        .into_iter()
        .zip(files)
        .map(|(found, sql)| {
            let found = found
                .into_iter()
                .filter(|finding| rules.is_enabled(finding.rule))
                .map(|finding| Diagnostic {
                    rule: finding.rule,
                    message: finding.message,
                    severity: rules.severity(finding.rule),
                    line: sql[..finding.span.start].matches('\n').count() + 1,
                    span: finding.span,
                    fix: Vec::new(),
                })
                .collect();
            apply_sql(&sql_ignores(sql), found, rules)
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
    }
}
