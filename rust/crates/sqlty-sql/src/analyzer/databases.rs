//! Named databases: a query picks the catalog and dialect it is read against with a
//! `sqlty:db=NAME` token on its first line, `--sql sqlty:db=analytics` in a string or
//! `-- sqlty: db=analytics` atop a `.sql` file.
//!
//! A schema loaded with [`Databases`] knows its own name and the other databases of the
//! project; [`route`] turns `(query, dialect, schema)` into the pair the query names.

use std::sync::{Arc, LazyLock};

use regex::Regex;

use super::suggest::closest;
use super::{Schema, SchemaData};

/// The name of the top-level database, which a marker may name like any other.
pub const DEFAULT: &str = "default";

/// The databases a schema routes queries to.
#[derive(Default)]
pub struct Databases {
    /// The name of the database the schema is.
    pub name: String,
    /// Whether the database has no schema: its queries are read without one.
    pub absent: bool,
    /// The other databases: name, dialect, and schema unless it has none.
    pub others: Vec<(String, &'static str, Option<Schema>)>,
}

/// The database named on the first line of `sql`, a `--` comment holding `sqlty:` and then
/// `db=NAME`; a line break may be escaped, as in source text.
pub fn database_name(sql: &str) -> Option<&str> {
    static NAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\A\s*--[^\n\\]*?\bsqlty:\s*db=([A-Za-z0-9_-]+)").expect("valid pattern")
    });
    Some(NAME.captures(sql)?.get(1)?.as_str())
}

/// The dialect and schema `sql` is read against: those of the database its first line
/// names, else `dialect` and `schema`. A schema loaded without [`Databases`] is kept.
pub fn route<'s>(
    sql: &str,
    dialect: &'s str,
    schema: Option<&'s SchemaData>,
) -> (&'s str, Option<&'s SchemaData>) {
    match schema {
        Some(catalog) => route_named(database_name(sql), dialect, catalog),
        None => (dialect, None),
    }
}

/// [`route`] for a query of the database `name`, `None` for the schema's own.
pub fn route_named<'s>(
    name: Option<&str>,
    dialect: &'s str,
    schema: &'s SchemaData,
) -> (&'s str, Option<&'s SchemaData>) {
    let Some(databases) = schema.databases() else {
        return (dialect, Some(schema));
    };
    let other = name
        .filter(|&name| name != databases.name)
        .and_then(|name| databases.others.iter().find(|(other, ..)| other == name));
    match other {
        Some((_, dialect, schema)) => (dialect, schema.as_deref()),
        None => (dialect, (!databases.absent).then_some(schema)),
    }
}

/// Why the database `sql` names is not one of `schema`'s: the message of
/// `unknown-database`, with the closest name when one is close. Without a schema or
/// [`Databases`], only `default` is known.
pub fn unknown_database(sql: &str, schema: Option<&SchemaData>) -> Option<String> {
    unknown_named(database_name(sql)?, schema)
}

/// [`unknown_database`] for the database `name`.
pub fn unknown_named(name: &str, schema: Option<&SchemaData>) -> Option<String> {
    let databases = schema.and_then(SchemaData::databases);
    let mut known = vec![databases.map_or(DEFAULT, |databases| databases.name.as_str())];
    known.extend(
        databases
            .into_iter()
            .flat_map(|databases| databases.others.iter().map(|(other, ..)| other.as_str())),
    );
    if known.contains(&name) {
        return None;
    }
    let mut message = format!("database `{name}` is not configured");
    if let Some(shown) = closest(name, &known) {
        message.push_str(&format!("; did you mean `{shown}`?"));
    }
    Some(message)
}

impl SchemaData {
    /// The databases this schema routes queries to, when it was loaded with them.
    pub fn databases(&self) -> Option<&Databases> {
        self.databases.as_deref()
    }

    /// This schema, routing queries to `databases`. `None` when it is shared already.
    pub fn with_databases(mut schema: Schema, databases: Databases) -> Option<Schema> {
        Arc::get_mut(&mut schema)?.databases = Some(Arc::new(databases));
        Some(schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{infer_type, load_schema};

    fn project() -> Schema {
        let analytics = load_schema("CREATE TABLE events (n INTEGER NOT NULL)", "sqlite");
        SchemaData::with_databases(
            load_schema("CREATE TABLE users (id TEXT NOT NULL)", "postgres"),
            Databases {
                name: DEFAULT.to_string(),
                absent: false,
                others: vec![
                    ("analytics".to_string(), "sqlite", Some(analytics)),
                    ("cache".to_string(), "mysql", None),
                ],
            },
        )
        .unwrap()
    }

    #[test]
    fn the_first_line_names_the_database() {
        assert_eq!(
            database_name("--sql sqlty:db=analytics\nSELECT 1"),
            Some("analytics")
        );
        assert_eq!(
            database_name("--sql sqlty:db=a-b_1\\nSELECT 1"),
            Some("a-b_1")
        );
        assert_eq!(database_name("  -- sql sqlty:db=x\nSELECT 1"), Some("x"));
        assert_eq!(
            database_name("-- sqlty: db=x\n-- name: q\nSELECT 1"),
            Some("x")
        );
        assert_eq!(database_name("--sql\nSELECT 'sqlty:db=x'"), None);
        assert_eq!(database_name("--sql\\nSELECT 1 -- sqlty:db=x"), None);
        assert_eq!(database_name("SELECT 1 -- sqlty:db=x"), None);
    }

    #[test]
    fn a_query_is_read_against_the_database_it_names() {
        let schema = project();
        let infer = |sql| infer_type(sql, "postgres", Some(&*schema));
        assert_eq!(
            infer("--sql\nSELECT id FROM users").as_deref(),
            Some("tuple[str]")
        );
        assert_eq!(
            infer("--sql sqlty:db=analytics\nSELECT n FROM events").as_deref(),
            Some("tuple[int]")
        );
        assert_eq!(
            infer("--sql sqlty:db=default\nSELECT id FROM users").as_deref(),
            Some("tuple[str]")
        );
        let (dialect, routed) = route("--sql sqlty:db=cache\nSELECT 1", "postgres", Some(&*schema));
        assert_eq!((dialect, routed.is_none()), ("mysql", true));
    }

    #[test]
    fn an_unknown_name_is_read_against_the_schema_itself() {
        let schema = project();
        let sql = "--sql sqlty:db=analytic\nSELECT id FROM users";
        assert_eq!(route(sql, "postgres", Some(&*schema)).0, "postgres");
        assert_eq!(
            unknown_database(sql, Some(&*schema)).as_deref(),
            Some("database `analytic` is not configured; did you mean `analytics`?")
        );
        assert_eq!(
            unknown_database("--sql sqlty:db=cache\nSELECT 1", Some(&*schema)),
            None
        );
        assert_eq!(
            unknown_database("--sql sqlty:db=default\nSELECT 1", None),
            None
        );
        assert!(unknown_database("--sql sqlty:db=x\nSELECT 1", None).is_some());
    }

    #[test]
    fn a_shared_schema_takes_no_databases() {
        let schema = project();
        let _shared = Arc::clone(&schema);
        assert!(SchemaData::with_databases(schema, Databases::default()).is_none());
    }
}
