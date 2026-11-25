//! The settings of one file: the database of `[tool.sqlty.databases]` it belongs to, and
//! the rules of the `[[tool.sqlty.overrides]]` that match it.

use std::path::Path;

use toml::{Table, Value};

use super::glob::{self, Excludes};
use super::Files;

/// Where a file of a project stands.
#[derive(Debug, Default, PartialEq)]
pub struct Route {
    /// The first database whose `queries` name the file or whose `include` matches it
    /// and `exclude` does not, unless the project's own `queries` name it.
    pub database: Option<String>,
    /// The rules of the matching overrides, a later one winning.
    pub rules: Table,
}

/// The route of the file at `path` in the project of `table`, whose paths are relative to
/// `base`.
pub fn route(
    table: &Table,
    base: &Path,
    path: &Path,
    files: &dyn Files,
    excludes: &Excludes,
) -> Route {
    let resolved = files.canonical(path);
    let base = files.canonical(base);
    let matches = |globs: &[String]| excludes.path(&resolved, &base, globs);
    let applies = |entry: &Table| {
        let exclude = strings(entry, "exclude");
        exclude.is_empty() || !matches(&exclude)
    };
    let own = strings(table, "queries");
    let mut found = Route::default();
    if !names_query_file(path, &resolved, &base, &own) {
        let databases = table.get("databases").and_then(Value::as_table);
        found.database = databases
            .into_iter()
            .flatten()
            .find_map(|(name, database)| {
                let database = database.as_table()?;
                let queries = match database.contains_key("queries") {
                    true => strings(database, "queries"),
                    false => own.clone(),
                };
                let named = names_query_file(path, &resolved, &base, &queries);
                (named || (matches(&strings(database, "include")) && applies(database)))
                    .then(|| name.clone())
            });
    }
    let overrides = table.get("overrides").and_then(Value::as_array);
    for entry in overrides.into_iter().flatten().filter_map(Value::as_table) {
        let include = strings(entry, "include");
        if (include.is_empty() || matches(&include)) && applies(entry) {
            let rules = entry.get("rules").and_then(Value::as_table).cloned();
            crate::config::merge_rules(&mut found.rules, rules.unwrap_or_default());
        }
    }
    found
}

/// `table` as the file of `route` reads it: its database's keys on top, `rules` and
/// `types` merged, then the rules of its overrides.
pub fn file_table(table: &Table, route: &Route) -> Table {
    let mut merged = table.clone();
    merged.remove("databases");
    merged.remove("overrides");
    let database = route.database.as_ref().and_then(|name| {
        let databases = table.get("databases")?.as_table()?;
        databases.get(name)?.as_table()
    });
    for (key, value) in database.into_iter().flatten() {
        match (merged.get_mut(key), value) {
            (Some(Value::Table(own)), Value::Table(value))
                if matches!(key.as_str(), "rules" | "types" | "deprecated-columns") =>
            {
                match key == "rules" {
                    true => crate::config::merge_rules(own, value.clone()),
                    false => own.extend(value.clone()),
                }
            }
            _ => {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    if !route.rules.is_empty() {
        let mut rules = merged
            .get("rules")
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default();
        crate::config::merge_rules(&mut rules, route.rules.clone());
        merged.insert("rules".into(), Value::Table(rules));
    }
    merged
}

/// Whether `queries` (relative to the resolved `base`) names the `.sql` file at `path`,
/// `resolved`: as the file, a directory holding it, or a glob.
pub fn names_query_file(path: &Path, resolved: &Path, base: &Path, queries: &[String]) -> bool {
    if queries.is_empty() || path.extension().is_none_or(|extension| extension != "sql") {
        return false;
    }
    let Ok(relative) = resolved.strip_prefix(base) else {
        return false;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    queries.iter().any(|entry| {
        let trimmed = entry.trim_matches('/');
        relative == trimmed
            || relative.starts_with(&format!("{trimmed}/"))
            || glob::path_prefix(entry).is_match(&relative)
    })
}

/// A key holding a string or a list of them.
fn strings(table: &Table, key: &str) -> Vec<String> {
    match table.get(key) {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::project::Read;

    fn routed(path: &str) -> (Route, Table) {
        let table: Table = toml::from_str(
            "queries = ['own']\nrules = {a = 'warn'}\ntypes = {DATE = 'str'}\n\
             [databases.logs]\nqueries = 'sql/logs/**/*.sql'\ninclude = ['logs']\nexclude = ['logs/old']\n\
             rules = {b = 'error'}\ntypes = {TIME = 'str'}\ndialect = 'duckdb'\n\
             [databases.rest]\ninclude = ['**/*.py']\n\
             [[overrides]]\ninclude = ['tests']\nrules = {a = 'ignore'}\n\
             [[overrides]]\ninclude = []\nexclude = ['tests/keep.py']\nrules = {c = 'warn'}\n",
        )
        .unwrap();
        let files = |_: &Path| Read::Missing;
        let route = route(
            &table,
            Path::new("/p"),
            Path::new(path),
            &files,
            &Excludes::default(),
        );
        let effective = file_table(&table, &route);
        (route, effective)
    }

    #[test]
    fn a_file_takes_its_database_and_overrides() {
        let (route, table) = routed("/p/logs/a.py");
        assert_eq!(route.database.as_deref(), Some("logs"));
        assert_eq!(table["dialect"].as_str(), Some("duckdb"));
        assert_eq!(
            table["types"].to_string(),
            "{ DATE = \"str\", TIME = \"str\" }"
        );
        assert_eq!(
            table["rules"].to_string(),
            "{ a = \"warn\", b = \"error\", c = \"warn\" }"
        );
        assert!(!table.contains_key("databases") && !table.contains_key("overrides"));

        assert_eq!(
            routed("/p/sql/logs/x/q.sql").0.database.as_deref(),
            Some("logs")
        );
        assert_eq!(routed("/p/own/q.sql").0.database, None);
        assert_eq!(
            routed("/p/logs/old/a.py").0.database.as_deref(),
            Some("rest")
        );
        assert_eq!(routed("/elsewhere/a.py").0.database, None);
        let (route, table) = routed("/p/tests/keep.py");
        assert_eq!(route.rules.to_string(), "a = \"ignore\"\n");
        assert_eq!(table["rules"].to_string(), "{ a = \"ignore\" }");
        let plain = file_table(&Table::new(), &Route::default());
        assert!(!plain.contains_key("rules"));
    }

    #[test]
    fn queries_name_files_directories_and_globs() {
        let named = |path: &str, queries: &[&str]| {
            let queries: Vec<String> = queries.iter().map(ToString::to_string).collect();
            names_query_file(Path::new(path), Path::new(path), Path::new("/p"), &queries)
        };
        assert!(named("/p/q.sql", &["q.sql"]));
        assert!(named("/p/sql/a/q.sql", &["/sql/"]));
        assert!(named("/p/sql/a/q.sql", &["sql/**"]));
        assert!(!named("/p/sql/a/q.sql", &["sql/*.sql"]));
        assert!(!named("/p/q.py", &["*"]));
        assert!(!named("/p/q.sql", &[]));
        assert!(!named("/x/q.sql", &["*"]));
    }
}
