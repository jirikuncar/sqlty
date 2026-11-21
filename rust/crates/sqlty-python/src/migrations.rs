//! The migration checks of `sqlty breaking`, with the user's severities and suppressions.

use crate::checker::Diagnostic;
use crate::rules::{Rule, RuleConfig};
use crate::suppress::{apply_sql, sql_ignores};

/// The diagnostics of each of `files`, PostgreSQL migrations in the order they run, for the
/// statements `baseline` does not have. `-- sqlty: ignore[rule]` on a statement's first line
/// drops its diagnostic.
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
            let newlines: Vec<usize> = sql.match_indices('\n').map(|(at, _)| at).collect();
            let found = found
                .into_iter()
                .filter(|finding| rules.is_enabled(finding.rule))
                .map(|finding| Diagnostic {
                    rule: finding.rule,
                    message: finding.message,
                    severity: rules.severity(finding.rule),
                    line: newlines.partition_point(|&at| at < finding.span.start) + 1,
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
