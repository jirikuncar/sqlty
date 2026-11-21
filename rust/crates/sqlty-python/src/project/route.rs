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
            found.rules.extend(rules.unwrap_or_default());
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
            (Some(Value::Table(own)), Value::Table(value)) if key == "rules" || key == "types" => {
                own.extend(value.clone());
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
        rules.extend(route.rules.clone());
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
