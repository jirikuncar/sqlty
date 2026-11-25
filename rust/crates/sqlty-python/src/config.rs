//! The `[tool.sqlty]` table: read from a configuration file's bytes and validated, so every
//! host reports an unknown key, rule or severity with the same message.
//!
//! The host finds the file (the nearest of [`CONFIG_FILES`]) and reads it; this module
//! never touches the filesystem. [`load`] picks the sqlty table of a native file
//! (`pyproject.toml`, `Cargo.toml`) and [`validate`] checks it, returning the table with its
//! TOML keys. A host checks what only it can read, such as the Python type expressions of
//! `types`.
//!
//! ```
//! use sqlty_python::config::{load, LoadError};
//!
//! let config = load(b"[tool.sqlty]\ndialect = \"mysql\"\n", "pyproject.toml", "pyproject.toml", true)
//!     .unwrap()
//!     .unwrap();
//! assert_eq!(config.table["dialect"].as_str(), Some("mysql"));
//!
//! let typo = load(b"dialet = \"mysql\"\n", "sqlty.toml", "sqlty.toml", true).unwrap_err();
//! assert!(matches!(typo, LoadError::Invalid(_)));
//! assert!(typo.to_string().contains("did you mean 'dialect'?"));
//! ```

use toml::{Table, Value};

use crate::rules::{Rule, Severity};
use sqlty_sql::Dialect;

/// The files a host looks for in each directory, in this order; the closest one with a
/// sqlty table wins.
pub const CONFIG_FILES: [&str; 3] = ["sqlty.toml", "pyproject.toml", "Cargo.toml"];

/// Keys of `[tool.sqlty]` that are not file discovery; see also [`DISCOVERY_KEYS`].
pub const TOOL_KEYS: [&str; 20] = [
    "dialect",
    "driver",
    "schema",
    "queries",
    "query-format",
    "rules",
    "overrides",
    "import-style",
    "untyped",
    "json-types",
    "param-types",
    "preview",
    "max-query-variants",
    "coerce-row-types",
    "functions",
    "types",
    "deprecated-columns",
    "format-command",
    "extend",
    "databases",
];

/// The `types` a `driver` adds in PostgreSQL, under the project's own: psycopg loads ranges
/// and multiranges as its own classes
/// (<https://www.psycopg.org/psycopg3/docs/basic/pgtypes.html>).
pub fn driver_types(driver: &str, dialect: &str) -> Vec<(String, String)> {
    const RANGES: [(&str, &str); 6] = [
        ("INT4", "int"),
        ("INT8", "int"),
        ("NUM", "decimal.Decimal"),
        ("DATE", "datetime.date"),
        ("TS", "datetime.datetime"),
        ("TSTZ", "datetime.datetime"),
    ];
    if driver != "psycopg" || dialect != "postgres" {
        return Vec::new();
    }
    RANGES
        .iter()
        .flat_map(|(base, python)| {
            [
                (
                    format!("{base}RANGE"),
                    format!("psycopg.types.range.Range[{python}]"),
                ),
                (
                    format!("{base}MULTIRANGE"),
                    format!("psycopg.types.multirange.Multirange[{python}]"),
                ),
            ]
        })
        .collect()
}

/// Keys of `[tool.sqlty]` that pick the files to process.
pub const DISCOVERY_KEYS: [&str; 3] = ["exclude", "extend-exclude", "respect-gitignore"];

/// Keys of a `[[tool.sqlty.functions]]` entry.
pub const FUNCTION_KEYS: [&str; 3] = ["import-path", "query-param", "return-type-param"];

/// Keys of a `[[tool.sqlty.overrides]]` entry.
pub const OVERRIDE_KEYS: [&str; 3] = ["include", "exclude", "rules"];

/// Keys of a `[tool.sqlty.databases.NAME]` table: what names a catalog, and the files it
/// applies to. The rest of `[tool.sqlty]` is shared by every database.
pub const DATABASE_KEYS: [&str; 10] = [
    "dialect",
    "schema",
    "queries",
    "query-format",
    "rules",
    "types",
    "deprecated-columns",
    "functions",
    "include",
    "exclude",
];

const REMOVED_KEYS: [(&str, &str); 1] = [(
    "type-modules",
    "removed, since `check` reads row types from source and never imports a module",
)];

/// A validated sqlty table.
#[derive(Debug, PartialEq)]
pub struct Config {
    /// The table with its TOML keys; every `overrides` entry has `include`, `exclude` and
    /// `rules`, and `functions` keeps only the entries with `import-path` and `query-param`.
    pub table: Table,
    /// The `functions` entries left out, rendered as TOML, for the host to warn about.
    pub skipped_functions: Vec<String>,
}

/// A key, rule, severity or value the configuration cannot have; the message names the
/// source.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct InvalidConfig(pub String);

/// Why a configuration file could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    /// The file is not UTF-8 or not TOML.
    #[error("{0}")]
    Syntax(String),
    /// The table is invalid, or a required native file has none.
    #[error(transparent)]
    Invalid(#[from] InvalidConfig),
}

/// Load the sqlty table of a file called `file_name`, which `source` names in messages.
///
/// `pyproject.toml` holds it in `[tool.sqlty]`, `Cargo.toml` in `[package.metadata.sqlty]`
/// or else `[workspace.metadata.sqlty]`; any other file holds it at the top level. A native
/// file without the table is `Ok(None)`, or an error when `required`.
pub fn load(
    data: &[u8],
    file_name: &str,
    source: &str,
    required: bool,
) -> Result<Option<Config>, LoadError> {
    let syntax = |message: String| LoadError::Syntax(format!("{source}: {message}"));
    let text = std::str::from_utf8(data).map_err(|error| syntax(error.to_string()))?;
    let mut document: Table = text
        .parse()
        .map_err(|error: toml::de::Error| syntax(error.to_string()))?;
    let paths: &[&[&str]] = match file_name {
        "pyproject.toml" => &[&["tool", "sqlty"]],
        "Cargo.toml" => &[
            &["package", "metadata", "sqlty"],
            &["workspace", "metadata", "sqlty"],
        ],
        _ => return Ok(Some(validate(document, source)?)),
    };
    for path in paths {
        if let Some(table) = take(&mut document, path) {
            return Ok(Some(validate(table, source)?));
        }
    }
    if !required {
        return Ok(None);
    }
    let tables: Vec<String> = paths
        .iter()
        .map(|path| format!("[{}]", path.join(".")))
        .collect();
    Err(InvalidConfig(format!("{source}: no {} table", tables.join(" or "))).into())
}

/// Check a sqlty table: unknown keys, rules and severities (with the closest name
/// suggested) and values of the wrong type are errors.
pub fn validate(mut table: Table, source: &str) -> Result<Config, InvalidConfig> {
    let fail = |message: String| Err(InvalidConfig(format!("{source}: {message}")));
    let named = |message: String| InvalidConfig(format!("{source}: {message}"));
    let keys: Vec<&str> = TOOL_KEYS.iter().chain(&DISCOVERY_KEYS).copied().collect();
    for key in table.keys() {
        if let Some((_, why)) = REMOVED_KEYS.iter().find(|(removed, _)| removed == key) {
            return fail(format!("unknown key '{key}' in [tool.sqlty]: {why}"));
        }
        if !keys.contains(&key.as_str()) {
            return fail(format!(
                "unknown key '{key}' in [tool.sqlty]{}; expected one of: {}",
                suggest(key, &keys),
                keys.join(", ")
            ));
        }
    }
    for key in ["param-types", "preview", "respect-gitignore"] {
        if table.get(key).is_some_and(|value| !value.is_bool()) {
            return fail(format!("[tool.sqlty] {key} must be true or false"));
        }
    }
    if table
        .get("max-query-variants")
        .is_some_and(|limit| limit.as_integer().is_none_or(|limit| limit < 1))
    {
        return fail("[tool.sqlty] max-query-variants must be a positive integer".into());
    }
    for key in ["dialect", "import-style", "extend"] {
        if table.get(key).is_some_and(|value| !value.is_str()) {
            return fail(format!("[tool.sqlty] {key} must be a string"));
        }
    }
    if table
        .get("driver")
        .is_some_and(|driver| driver.as_str() != Some("psycopg"))
    {
        return fail("[tool.sqlty] driver must be \"psycopg\"".into());
    }
    if let Some(dialect) = table.get("dialect").and_then(Value::as_str) {
        if dialect.parse::<Dialect>().is_err() {
            return fail(format!(
                "[tool.sqlty] unknown dialect '{dialect}'{}; expected one of: {}",
                suggest(dialect, &Dialect::NAMES),
                Dialect::NAMES.join(", ")
            ));
        }
    }
    for key in ["schema", "queries"] {
        if table
            .get(key)
            .is_some_and(|value| !value.is_str() && !is_strings(value))
        {
            return fail(format!(
                "[tool.sqlty] {key} must be a path or a list of paths"
            ));
        }
    }
    if let Some(format) = table.get("query-format") {
        let names = crate::queries::Format::NAMES;
        match format.as_str() {
            Some(name) if name.parse::<crate::queries::Format>().is_ok() => {}
            Some(name) => {
                return fail(format!(
                    "[tool.sqlty] unknown query-format '{name}'{}; expected one of: {}",
                    suggest(name, &names),
                    names.join(", ")
                ))
            }
            None => return fail("[tool.sqlty] query-format must be a string".into()),
        }
    }
    if let Some(rules) = table.get("rules") {
        check_rules(rules, "[tool.sqlty] rules", "[tool.sqlty.rules]").map_err(named)?;
    }
    if let Some(overrides) = table.get_mut("overrides") {
        let Some(overrides) = overrides
            .as_array_mut()
            .filter(|o| o.iter().all(Value::is_table))
        else {
            return fail("[tool.sqlty] overrides must be an array of tables".into());
        };
        for entry in overrides.iter_mut().filter_map(Value::as_table_mut) {
            if let Some(key) = entry
                .keys()
                .find(|key| !OVERRIDE_KEYS.contains(&key.as_str()))
            {
                return fail(format!(
                    "unknown key '{key}' in [[tool.sqlty.overrides]]{}; expected one of: {}",
                    suggest(key, &OVERRIDE_KEYS),
                    OVERRIDE_KEYS.join(", ")
                ));
            }
            for key in ["include", "exclude"] {
                if !is_strings(entry.entry(key).or_insert_with(|| Value::Array(vec![]))) {
                    return fail(format!(
                        "[[tool.sqlty.overrides]] {key} must be a list of glob strings"
                    ));
                }
            }
            let rules = entry
                .entry("rules")
                .or_insert_with(|| Value::Table(Table::new()));
            check_rules(
                rules,
                "[[tool.sqlty.overrides]] rules",
                "[tool.sqlty.overrides.rules]",
            )
            .map_err(named)?;
        }
    }
    let mut skipped_functions = vec![];
    if let Some(databases) = table.get_mut("databases") {
        let Some(databases) = databases
            .as_table_mut()
            .filter(|d| d.values().all(Value::is_table))
        else {
            return fail("[tool.sqlty] databases must be a table of tables".into());
        };
        for (name, entry) in databases
            .iter_mut()
            .filter_map(|(name, database)| Some((name, database.as_table_mut()?)))
        {
            let place = format!("[tool.sqlty.databases.{name}]");
            if name == "default"
                || name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return fail(format!(
                    "{place}: a database name is letters, digits, '-' and '_', and not 'default'"
                ));
            }
            if let Some(key) = entry
                .keys()
                .find(|key| !DATABASE_KEYS.contains(&key.as_str()))
            {
                return fail(format!(
                    "unknown key '{key}' in {place}{}; expected one of: {}",
                    suggest(key, &DATABASE_KEYS),
                    DATABASE_KEYS.join(", ")
                ));
            }
            let mut globs = Table::new();
            for key in ["include", "exclude"] {
                let value = entry.remove(key).unwrap_or(Value::Array(vec![]));
                if !is_strings(&value) {
                    return fail(format!("{place} {key} must be a list of glob strings"));
                }
                globs.insert(key.into(), value);
            }
            let checked = validate(std::mem::take(entry), source).map_err(|error| {
                InvalidConfig(
                    error
                        .0
                        .replace("[tool.sqlty.", &format!("[tool.sqlty.databases.{name}."))
                        .replace("[tool.sqlty]", &place),
                )
            })?;
            skipped_functions.extend(checked.skipped_functions);
            *entry = checked.table;
            entry.extend(globs);
        }
    }
    if let Some(functions) = table.remove("functions") {
        let Value::Array(functions) = functions else {
            return fail("[tool.sqlty] functions must be an array of tables".into());
        };
        let mut kept = vec![];
        for function in functions {
            let Some(entry) = function.as_table() else {
                skipped_functions.push(function.to_string());
                continue;
            };
            for (key, value) in entry {
                if !FUNCTION_KEYS.contains(&key.as_str()) {
                    return fail(format!(
                        "unknown key '{key}' in [[tool.sqlty.functions]]{}; expected one of: {}",
                        suggest(key, &FUNCTION_KEYS),
                        FUNCTION_KEYS.join(", ")
                    ));
                }
                if !value.is_str() {
                    return fail(format!("[[tool.sqlty.functions]] {key} must be a string"));
                }
            }
            if entry.contains_key("import-path") && entry.contains_key("query-param") {
                kept.push(function);
            } else {
                skipped_functions.push(function.to_string());
            }
        }
        if !kept.is_empty() {
            table.insert("functions".into(), Value::Array(kept));
        }
    }
    if let Some(columns) = table.get("deprecated-columns") {
        check_deprecated(columns).map_err(named)?;
    }
    if table.get("types").is_some_and(|types| {
        types
            .as_table()
            .is_none_or(|types| !types.values().all(Value::is_str))
    }) {
        return fail(
            "[tool.sqlty] types must be a table of SQL type or table.column = Python type, \
             such as DATE = \"datetime.date\""
                .into(),
        );
    }
    if let Some((key, python)) = table
        .get("types")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(key, python)| Some((key, python.as_str()?)))
        .find(|(_, python)| crate::parse::expression(python).is_none())
    {
        return fail(format!(
            "[tool.sqlty.types] {key} = {} is not a Python type expression",
            crate::unparse::repr_str(python)
        ));
    }
    if table
        .get("format-command")
        .is_some_and(|command| !is_strings(command))
    {
        return fail(
            "[tool.sqlty] format-command must be a list of strings, such as [\"ruff\", \"format\"]"
                .into(),
        );
    }
    if table
        .get("coerce-row-types")
        .is_some_and(|names| !is_strings(names))
    {
        return fail(
            "[tool.sqlty] coerce-row-types must be a list of row type names, such as [\"app.rows.User\"]"
                .into(),
        );
    }
    for key in ["exclude", "extend-exclude"] {
        if table.get(key).is_some_and(|globs| !is_strings(globs)) {
            return fail(format!("[tool.sqlty] {key} must be a list of glob strings"));
        }
    }
    Ok(Config {
        table,
        skipped_functions,
    })
}

fn check_rules(rules: &Value, key: &str, place: &str) -> Result<(), String> {
    let Some(rules) = rules.as_table() else {
        return Err(format!("{key} must be a table of rule = severity"));
    };
    let names: Vec<&str> = Rule::ALL.iter().map(|rule| rule.name()).collect();
    for (rule, value) in rules {
        if Rule::parse(rule).is_none() {
            return Err(format!(
                "unknown rule '{rule}' in {place}{}; run `sqlty rule` for the list",
                suggest(rule, &names)
            ));
        }
        let options = match rule.as_str() {
            "missing-lock-timeout" => &["severity", "max"][..],
            _ => &["severity"][..],
        };
        let severity = match value.as_table() {
            Some(table) => {
                if let Some(key) = table.keys().find(|key| !options.contains(&key.as_str())) {
                    return Err(format!(
                        "unknown key '{key}' in {}.{rule}]; expected one of: {}",
                        place.trim_end_matches(']'),
                        options.join(", ")
                    ));
                }
                if let Some(max) = table.get("max").filter(|max| {
                    max.as_str()
                        .and_then(sqlty_sql::migrations::lock_timeout_ms)
                        .is_none()
                }) {
                    let max = max
                        .as_str()
                        .map_or_else(|| max.to_string(), |s| format!("'{s}'"));
                    return Err(format!(
                        "invalid max {max} for rule '{rule}' in {place}; expected a duration such as '2s'"
                    ));
                }
                table.get("severity")
            }
            None => Some(value),
        };
        if let Some(severity) = severity.filter(|s| s.as_str().and_then(Severity::parse).is_none())
        {
            let severity = severity
                .as_str()
                .map_or_else(|| severity.to_string(), |s| format!("'{s}'"));
            return Err(format!(
                "invalid severity {severity} for rule '{rule}' in {place}; expected one of: error, warn, ignore"
            ));
        }
    }
    Ok(())
}

/// `over` merged into the `rules` table `under`, rule by rule: a severity, plain or in
/// `severity`, keeps the options `under` gives the rule, and an option keeps its severity.
///
/// Example:
///
/// ```
/// use sqlty_python::config::merge_rules;
///
/// let mut under: toml::Table = "[missing-lock-timeout]\nmax = '2s'".parse().unwrap();
/// merge_rules(&mut under, "missing-lock-timeout = 'error'".parse().unwrap());
/// assert_eq!(under.to_string(), "[missing-lock-timeout]\nmax = \"2s\"\nseverity = \"error\"\n");
/// ```
pub fn merge_rules(under: &mut Table, over: Table) {
    let options = |value: Value| match value {
        Value::Table(options) => options,
        severity => Table::from_iter([("severity".to_string(), severity)]),
    };
    for (rule, value) in over {
        let merged = match under.remove(&rule) {
            Some(Value::Table(mut own)) => {
                own.extend(options(value));
                Value::Table(own)
            }
            Some(severity) if value.is_table() => {
                let mut own = options(severity);
                own.extend(options(value));
                Value::Table(own)
            }
            _ => value,
        };
        under.insert(rule, merged);
    }
}

/// The entries of a checked `rules` table as hosts pass them, rule to severity: a rule's
/// table gives its `severity`, and its `max` as `missing-lock-timeout.max`.
pub fn rule_entries(rules: Option<&Value>) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for (rule, value) in rules.and_then(Value::as_table).into_iter().flatten() {
        let Some(table) = value.as_table() else {
            entries.extend(
                value
                    .as_str()
                    .map(|severity| (rule.clone(), severity.to_string())),
            );
            continue;
        };
        for (key, value) in table {
            let name = match key.as_str() {
                "severity" => rule.clone(),
                _ => format!("{rule}.{key}"),
            };
            entries.extend(value.as_str().map(|value| (name, value.to_string())));
        }
    }
    entries
}

/// Keys of a `[tool.sqlty.deprecated-columns]` entry.
pub const DEPRECATED_KEYS: [&str; 2] = ["use", "remove-after"];

fn check_deprecated(columns: &Value) -> Result<(), String> {
    let place = "[tool.sqlty.deprecated-columns]";
    let Some(columns) = columns.as_table() else {
        return Err(format!(
            "{place} must be a table of table.column = {{ use = \"column\", remove-after = \"date\" }}"
        ));
    };
    for (key, entry) in columns {
        if !key.contains('.') {
            return Err(format!("{place} '{key}' must be table.column"));
        }
        let Some(entry) = entry.as_table() else {
            return Err(format!(
                "{place} '{key}' must be a table of use and remove-after"
            ));
        };
        for (option, value) in entry {
            if !DEPRECATED_KEYS.contains(&option.as_str()) {
                return Err(format!(
                    "unknown key '{option}' in {place} '{key}'{}; expected one of: {}",
                    suggest(option, &DEPRECATED_KEYS),
                    DEPRECATED_KEYS.join(", ")
                ));
            }
            if !value.is_str() {
                return Err(format!("{place} '{key}' {option} must be a string"));
            }
        }
    }
    Ok(())
}

/// The `deprecated-columns` of a checked table as the `types` pairs
/// `TypeOverrides::new` reads.
pub fn deprecated_types(table: &Table) -> Vec<(String, String)> {
    let columns = table.get("deprecated-columns").and_then(Value::as_table);
    columns
        .into_iter()
        .flatten()
        .map(|(key, entry)| {
            let part = |name: &str| entry.get(name).and_then(Value::as_str);
            sqlty_sql::analyzer::deprecated_type(key, part("use"), part("remove-after"))
        })
        .collect()
}

fn is_strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_str))
}

fn take(document: &mut Table, path: &[&str]) -> Option<Table> {
    let (last, parents) = path.split_last()?;
    let mut table = document;
    for key in parents {
        table = table.get_mut(*key)?.as_table_mut()?;
    }
    match table.remove(*last)? {
        Value::Table(found) => Some(found),
        _ => None,
    }
}

/// ` (did you mean 'x'?)` for the choice most like `name`, as Python's
/// `difflib.get_close_matches(name, choices, n=1)` picks it, or nothing.
pub fn suggest(name: &str, choices: &[&str]) -> String {
    let name: Vec<char> = name.chars().collect();
    choices
        .iter()
        .map(|choice| (ratio(&name, &choice.chars().collect::<Vec<_>>()), *choice))
        .filter(|(score, _)| *score >= 0.6)
        .max_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)))
        .map(|(_, choice)| format!(" (did you mean '{choice}'?)"))
        .unwrap_or_default()
}

/// `difflib.SequenceMatcher(None, a, b).ratio()`: twice the matched characters over the
/// total. Junk heuristics never apply, since names are shorter than 200 characters.
fn ratio(a: &[char], b: &[char]) -> f64 {
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    2.0 * matched(a, b) as f64 / total as f64
}

/// Characters in the matching blocks: the longest common run (the earliest in `a`, then in
/// `b`), then the same on each side of it.
fn matched(a: &[char], b: &[char]) -> usize {
    let (mut best, mut start_a, mut start_b) = (0, 0, 0);
    for i in 0..a.len() {
        for j in 0..b.len() {
            let run = a[i..]
                .iter()
                .zip(&b[j..])
                .take_while(|(x, y)| x == y)
                .count();
            if run > best {
                (best, start_a, start_b) = (run, i, j);
            }
        }
    }
    if best == 0 {
        return 0;
    }
    best + matched(&a[..start_a], &b[..start_b])
        + matched(&a[start_a + best..], &b[start_b + best..])
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn invalid(toml: &str) -> String {
        validate(toml.parse().unwrap(), "sqlty.toml").unwrap_err().0
    }

    #[test]
    fn a_rule_table_gives_its_severity_and_options() {
        let config = validate(
            "[rules]\nselect-star = 'error'\nmissing-lock-timeout = { max = '2s' }\n\
             [rules.unknown-column]\nseverity = 'warn'"
                .parse()
                .unwrap(),
            "sqlty.toml",
        )
        .unwrap();
        assert_eq!(
            rule_entries(config.table.get("rules")),
            [
                ("select-star".to_string(), "error".to_string()),
                ("missing-lock-timeout.max".to_string(), "2s".to_string()),
                ("unknown-column".to_string(), "warn".to_string()),
            ]
        );
    }

    #[test]
    fn deprecated_columns_are_carried_with_the_types() {
        let table = "[deprecated-columns]\n'users.a' = { use = 'b', remove-after = 'soon' }\n\
                     'users.c' = {}";
        let table = validate(table.parse().unwrap(), "sqlty.toml")
            .unwrap()
            .table;
        assert_eq!(
            deprecated_types(&table),
            [
                ("!users.a".to_string(), "b\tsoon".to_string()),
                ("!users.c".to_string(), "\t".to_string()),
            ]
        );
    }

    #[test]
    fn rules_merge_rule_by_rule() {
        let merged = |under: &str, over: &str| {
            let mut under: Table = under.parse().unwrap();
            merge_rules(&mut under, over.parse().unwrap());
            rule_entries(Some(&Value::Table(under)))
        };
        let entries = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            let pair = |(key, value): &(&str, &str)| (key.to_string(), value.to_string());
            pairs.iter().map(pair).collect()
        };
        let max = "missing-lock-timeout = { max = '1s' }";
        let error = "missing-lock-timeout = 'error'";
        let both = [
            ("missing-lock-timeout.max", "1s"),
            ("missing-lock-timeout", "error"),
        ];
        assert_eq!(merged(max, error), entries(&both));
        assert_eq!(
            merged(error, max),
            entries(&[
                ("missing-lock-timeout", "error"),
                ("missing-lock-timeout.max", "1s")
            ])
        );
        assert_eq!(
            merged(error, "missing-lock-timeout = 'warn'"),
            entries(&[("missing-lock-timeout", "warn")])
        );
        assert_eq!(
            merged("", max),
            entries(&[("missing-lock-timeout.max", "1s")])
        );
    }

    #[test]
    fn suggests_like_difflib() {
        assert_eq!(suggest("dialet", &TOOL_KEYS), " (did you mean 'dialect'?)");
        assert_eq!(
            suggest("import_style", &TOOL_KEYS),
            " (did you mean 'import-style'?)"
        );
        assert_eq!(
            suggest("postgress", &Dialect::NAMES),
            " (did you mean 'postgres'?)"
        );
        assert_eq!(suggest("bogus", &TOOL_KEYS), "");
        assert_eq!(ratio(&['a', 'b', 'c', 'd'], &['b', 'c', 'd', 'e']), 0.75);
        assert_eq!(ratio(&[], &[]), 1.0);
    }

    #[test]
    fn reports_unknown_keys_rules_and_severities() {
        assert_eq!(
            invalid("type-modules = []"),
            "sqlty.toml: unknown key 'type-modules' in [tool.sqlty]: removed, since `check` \
             reads row types from source and never imports a module"
        );
        assert!(invalid("[rules]\nstale-typ = 'warn'").contains("did you mean 'stale-type'?"));
        assert!(invalid("[rules]\nstale-type = 'loud'").contains("invalid severity 'loud'"));
        assert!(invalid("[rules]\nstale-type = 1").contains("invalid severity 1"));
        assert!(invalid("[rules.missing-lock-timeout]\nmax = 2").contains("invalid max 2"));
        for (toml, message) in [
            (
                "deprecated-columns = 1",
                "deprecated-columns] must be a table",
            ),
            (
                "[deprecated-columns]\nemail = {}",
                "'email' must be table.column",
            ),
            (
                "[deprecated-columns]\n'u.e' = 'x'",
                "'u.e' must be a table of use",
            ),
            (
                "[deprecated-columns]\n'u.e' = { uses = 'f' }",
                "did you mean 'use'?",
            ),
            (
                "[deprecated-columns]\n'u.e' = { use = 1 }",
                "'u.e' use must be a string",
            ),
        ] {
            assert!(invalid(toml).contains(message), "{toml}");
        }
        assert_eq!(
            invalid("[rules.select-star]\nmax = '2s'"),
            "sqlty.toml: unknown key 'max' in [tool.sqlty.rules.select-star]; expected one of: severity"
        );
        assert!(
            invalid("[rules.missing-lock-timeout]\nseverity = 1").contains("invalid severity 1")
        );
        assert!(invalid("[[overrides]]\nrule = {}").contains("did you mean 'rules'?"));
        assert!(invalid("[[functions]]\nquery = 'q'").contains("unknown key 'query'"));
        assert!(invalid("dialect = 'postgress'").contains("did you mean 'postgres'?"));
        assert!(invalid("dialect = 1").contains("dialect must be a string"));
        assert!(invalid("extend = []").contains("extend must be a string"));
        assert!(invalid("schema = 1").contains("schema must be a path"));
        assert!(invalid("max-query-variants = 0").contains("positive integer"));
        assert!(invalid("coerce-row-types = \"app.User\"").contains("list of row type names"));
        assert!(invalid("driver = \"asyncpg\"").contains("must be \"psycopg\""));
        assert!(invalid("types = { DATE = 1 }").contains("types must be a table"));
        assert!(invalid("databases = 1").contains("databases must be a table of tables"));
        assert_eq!(
            invalid("types = { DATE = \"list[\" }"),
            "sqlty.toml: [tool.sqlty.types] DATE = 'list[' is not a Python type expression"
        );
        assert!(invalid("[databases.default]").contains("not 'default'"));
        assert!(invalid("[databases.'a b']").contains("letters, digits"));
        assert_eq!(
            invalid("[databases.analytics]\nimport-style = 'import sqlty'"),
            "sqlty.toml: unknown key 'import-style' in [tool.sqlty.databases.analytics]; \
             expected one of: dialect, schema, queries, query-format, rules, types, \
             deprecated-columns, functions, include, exclude"
        );
        assert!(invalid("[databases.a]\ninclude = 'x'").contains("include must be a list"));
        assert!(invalid("[databases.a]\ndialect = 'duck'").starts_with(
            "sqlty.toml: [tool.sqlty.databases.a] unknown dialect 'duck' (did you mean 'duckdb'?)"
        ));
    }

    #[test]
    fn fills_the_globs_of_each_database() {
        let config = validate(
            "[databases.analytics]\ndialect = 'duckdb'\nfunctions = [{ import-path = 'r' }]"
                .parse()
                .unwrap(),
            "sqlty.toml",
        )
        .unwrap();
        let expected: Table =
            "[databases.analytics]\ndialect = 'duckdb'\ninclude = []\nexclude = []"
                .parse()
                .unwrap();
        assert_eq!(config.table, expected);
        assert_eq!(config.skipped_functions, ["{ import-path = \"r\" }"]);
    }

    #[test]
    fn fills_overrides_and_skips_incomplete_functions() {
        let config = validate(
            "functions = ['junk', { import-path = 'db.run' }, { import-path = 'db.ok', query-param = 'q' }]\n\
             [[overrides]]\ninclude = ['a']"
                .parse()
                .unwrap(),
            "sqlty.toml",
        )
        .unwrap();
        let expected: Table = "functions = [{ import-path = 'db.ok', query-param = 'q' }]\n\
             [[overrides]]\ninclude = ['a']\nexclude = []\nrules = {}"
            .parse()
            .unwrap();
        assert_eq!(config.table, expected);
        assert_eq!(
            config.skipped_functions,
            ["\"junk\"", "{ import-path = \"db.run\" }"]
        );
    }

    #[test]
    fn loads_the_table_of_each_file() {
        let load = |data: &str, name| load(data.as_bytes(), name, name, false);
        let dialect = |config: Option<Config>| {
            config.unwrap().table["dialect"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(
            dialect(load("[tool.sqlty]\ndialect = 'mysql'", "pyproject.toml").unwrap()),
            "mysql"
        );
        assert_eq!(
            dialect(
                load(
                    "[workspace.metadata.sqlty]\ndialect = 'sqlite'",
                    "Cargo.toml"
                )
                .unwrap()
            ),
            "sqlite"
        );
        assert_eq!(load("[project]\nname = 'x'", "pyproject.toml"), Ok(None));
        assert_eq!(load("[package]\nmetadata = 1", "Cargo.toml"), Ok(None));
        assert!(matches!(
            load("[tool.sqlty", "pyproject.toml"),
            Err(LoadError::Syntax(_))
        ));
        assert!(matches!(
            super::load(b"\xff", "sqlty.toml", "x", true),
            Err(LoadError::Syntax(_))
        ));
        assert_eq!(
            super::load(b"", "Cargo.toml", "Cargo.toml", true)
                .unwrap_err()
                .to_string(),
            "Cargo.toml: no [package.metadata.sqlty] or [workspace.metadata.sqlty] table"
        );
    }

    #[test]
    fn rarer_configs() {
        assert!(invalid("query-format = 1").contains("query-format must be a string"));
        assert!(driver_types("psycopg", "mysql").is_empty());
        let loaded = load(
            b"[tool]\nsqlty = 1\n",
            "pyproject.toml",
            "pyproject.toml",
            false,
        );
        assert!(matches!(loaded, Ok(None)));
    }
}
