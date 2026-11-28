//! The migration checks of `sqlty breaking`, with the user's severities and suppressions.

use crate::checker::{query_sites, Diagnostic, SiteKind};
use crate::queries::{split, Format};
use crate::rewriter::{ImportStyle, LineIndex};
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
    check_migrations_with(baseline, &files, &[])
}

/// [`check_migrations`] with the rules of each file: `[(sql, rules)]`, the severities of
/// the overrides its path matches; and the queries of the code deployed with them,
/// `[(label, sql)]` as [`queries_of`] gives them.
pub fn check_migrations_with(
    baseline: &str,
    files: &[(&str, &RuleConfig)],
    writers: &[(String, String)],
) -> Vec<Vec<Diagnostic>> {
    let sources: Vec<(&str, Option<&str>)> = files
        .iter()
        .map(|(sql, rules)| (*sql, rules.lock_timeout_max.as_deref()))
        .collect();
    sqlty_sql::migrations::check_with(baseline, &sources, writers)
        .into_iter()
        .zip(files)
        .map(|(found, (sql, rules))| diagnosed(sql, found, rules))
        .collect()
}

/// The schema checks of `sqlty check` (`tenant-policy`) of each of `files`, whose texts
/// joined with `\n` are the SQL `schema` was loaded from, each with its rules.
/// `-- sqlty: ignore[rule]` drops a diagnostic as in [`check_migrations`].
///
/// Example:
///
/// ```
/// use sqlty_python::migrations::check_schema;
/// use sqlty_python::rules::RuleConfig;
/// use sqlty_sql::analyzer::load_schema_with_skipped;
///
/// let sql = "CREATE TABLE notes (org_id INT);\nALTER TABLE notes ENABLE ROW LEVEL SECURITY;\n\
///            CREATE POLICY t ON notes USING (org_id = current_setting('app.org')::int);";
/// let (schema, _) = load_schema_with_skipped(sql, "postgres");
/// let rules = RuleConfig::default();
/// let found = check_schema(&schema, &[(sql, &rules)]);
/// assert_eq!(found[0][0].line, 2);
/// ```
pub fn check_schema(
    schema: &sqlty_sql::analyzer::SchemaData,
    files: &[(&str, &RuleConfig)],
) -> Vec<Vec<Diagnostic>> {
    let mut findings = schema.tenant_policies().into_iter().peekable();
    let mut start = 0;
    files
        .iter()
        .map(|(sql, rules)| {
            let end = start + sql.len();
            let mut own = Vec::new();
            while let Some(found) = findings.next_if(|found| found.span.start <= end) {
                if found.span.start >= start && found.span.end <= end {
                    let span = found.span.start - start..found.span.end - start;
                    own.push(sqlty_sql::migrations::Finding { span, ..found });
                }
            }
            start = end + 1;
            diagnosed(sql, own, rules)
        })
        .collect()
}

/// The diagnostics of `found` in the SQL file `sql`, each spanning its statement, with the
/// file's severities and suppressions.
fn diagnosed(
    sql: &str,
    found: Vec<sqlty_sql::migrations::Finding>,
    rules: &RuleConfig,
) -> Vec<Diagnostic> {
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
            related: Vec::new(),
        })
        .collect();
    let statements: Vec<_> = found.iter().map(|found| found.span.clone()).collect();
    apply_sql(&sql_ignores(sql, &statements), found, rules)
        .into_iter()
        .filter(|diagnostic| diagnostic.rule != Rule::UnusedIgnore)
        .collect()
}

/// The queries of a Python module (`format` `None`) or a `.sql` query file in `format`,
/// shown as `path`, each as `(path:line, sql)`.
pub fn queries_of(
    path: &str,
    text: &str,
    style: &ImportStyle,
    format: Option<Format>,
) -> Vec<(String, String)> {
    let Some(format) = format else {
        let sites = query_sites(text, style, &[]);
        let queries = sites
            .into_iter()
            .filter(|(.., kind, _)| *kind == SiteKind::Query);
        return queries
            .map(|(line, _, sql)| (format!("{path}:{line}"), sql))
            .collect();
    };
    let lines = LineIndex::new(text);
    let stem = path.rsplit('/').next().unwrap_or(path);
    split(text, format, stem.strip_suffix(".sql").unwrap_or(stem))
        .into_iter()
        .map(|query| {
            (
                format!("{path}:{}", lines.line_at(query.body.start)),
                query.sql,
            )
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
    fn a_backfill_names_a_query_of_a_module_or_a_query_file() {
        let style = ImportStyle::default();
        let module =
            "from sqlty import SQL\nq = SQL(\"--sql\\nINSERT INTO users (id) VALUES (1)\")\n";
        let file = "-- name: rename!\nUPDATE users SET id = :id;\n";
        let mut writers = queries_of("app/q.py", module, &style, None);
        writers.extend(queries_of(
            "sql/users.sql",
            file,
            &style,
            Some(Format::Auto),
        ));
        let labels: Vec<&str> = writers.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["app/q.py:2", "sql/users.sql:2"]);
        let migration =
            "SET lock_timeout = '1s';\nUPDATE users SET bio = '' WHERE id BETWEEN 1 AND 9;";
        let found = check_migrations_with(BASELINE, &[(migration, &rules(&[]))], &writers);
        let [diagnostic] = found[0].as_slice() else {
            panic!("{found:?}");
        };
        assert_eq!(diagnostic.rule, Rule::BackfillWithoutWriter);
        assert!(
            diagnostic.message.contains("`app/q.py:2`"),
            "{}",
            diagnostic.message
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
        let found = check_migrations_with(BASELINE, &[(first, &warn), (second, &error)], &[]);
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
