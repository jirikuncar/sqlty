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
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| crate::regex(r"\A\s*--[^\n\\]*?\bsqlty:\s*db=([A-Za-z0-9_-]+)"));
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
