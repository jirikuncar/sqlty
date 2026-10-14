//! Name folding and the keys catalog maps are looked up by.
//!
//! A key is `schema.name` in PostgreSQL and `name` elsewhere. Nothing here reads a
//! [`super::Catalog`] or a loader; it only turns identifiers into keys.

use std::collections::HashMap;

use sqlparser::ast::{DataType, Ident, ObjectName};

use crate::analyzer::SqlDialect;

/// The key a bare name is tried under in each schema of `path`; PostgreSQL always
/// searches `pg_catalog` first.
pub(super) fn lookup_key<'a, T>(
    map: &'a HashMap<String, T>,
    path: &[String],
    name: &str,
) -> Option<&'a str> {
    if name.contains('.') {
        return map.get_key_value(name).map(|(key, _)| key.as_str());
    }
    for schema in std::iter::once("pg_catalog").chain(path.iter().map(String::as_str)) {
        if let Some((key, _)) = map.get_key_value(&format!("{schema}.{name}")) {
            return Some(key);
        }
    }
    None
}

/// The lookup name of a user-defined type such as `app.percent`.
pub(super) fn type_key(dt: &DataType, dialect: SqlDialect) -> Option<String> {
    match dt {
        DataType::Custom(name, modifiers) if modifiers.is_empty() => Some(name_key(name, dialect)),
        _ => None,
    }
}

/// An identifier as the dialect keys it. PostgreSQL folds an unquoted name to lower case and
/// keeps a quoted one exactly; MySQL and SQLite compare names case-insensitively.
pub(crate) fn fold(ident: &Ident, dialect: SqlDialect) -> String {
    if dialect == SqlDialect::Postgres && ident.quote_style.is_some() {
        ident.value.clone()
    } else {
        ident.value.to_lowercase()
    }
}

pub(super) fn parts(name: &ObjectName, dialect: SqlDialect) -> Vec<String> {
    name.0
        .iter()
        .filter_map(|part| part.as_ident())
        .map(|ident| fold(ident, dialect))
        .collect()
}

pub(super) fn key_of(parts: &[String], dialect: SqlDialect) -> String {
    let keep = if dialect == SqlDialect::Postgres {
        2
    } else {
        1
    };
    parts[parts.len().saturating_sub(keep)..].join(".")
}

/// The name a lookup uses: `schema.name` or `name` in PostgreSQL, `name` elsewhere.
pub(super) fn name_key(name: &ObjectName, dialect: SqlDialect) -> String {
    key_of(&parts(name, dialect), dialect)
}

/// Table-level key of a name seen under `path`.
pub(super) fn schema_for_new(path: &[String]) -> &str {
    path.iter()
        .map(String::as_str)
        .find(|schema| !matches!(*schema, "$user" | "pg_catalog" | "pg_temp"))
        .unwrap_or("public")
}
