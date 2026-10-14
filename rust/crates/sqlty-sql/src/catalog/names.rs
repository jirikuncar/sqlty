//! Name folding and the keys catalog maps are looked up by.
//!
//! A key is `schema.name` in PostgreSQL and `name` elsewhere. Nothing here reads a
//! [`super::Catalog`] or a loader; it only turns identifiers into keys.

use rustc_hash::FxHashMap;

use sqlparser::ast::{DataType, Ident, ObjectName};

use crate::analyzer::SqlDialect;

/// The key a bare name is tried under in each schema of `path`; PostgreSQL always
/// searches `pg_catalog` first.
pub(super) fn lookup_key<'a, T>(
    map: &'a FxHashMap<String, T>,
    path: &[String],
    name: &str,
) -> Option<&'a str> {
    if name.contains('.') {
        return map.get_key_value(name).map(|(key, _)| key.as_str());
    }
    let schemas = std::iter::once("pg_catalog").chain(path.iter().map(String::as_str));
    // `schema.name` is built in a stack buffer, so a probe does not allocate; a name too
    // long for it takes the heap.
    let mut buf = [0u8; 128];
    for schema in schemas {
        let len = schema.len() + 1 + name.len();
        let found = if len <= buf.len() {
            buf[..schema.len()].copy_from_slice(schema.as_bytes());
            buf[schema.len()] = b'.';
            buf[schema.len() + 1..len].copy_from_slice(name.as_bytes());
            // The pieces are `str`s joined by an ASCII dot, so this is always UTF-8.
            std::str::from_utf8(&buf[..len])
                .ok()
                .and_then(|key| map.get_key_value(key))
        } else {
            map.get_key_value(&format!("{schema}.{name}"))
        };
        if let Some((key, _)) = found {
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
    let keep = if dialect == SqlDialect::Postgres {
        2
    } else {
        1
    };
    let idents = || name.0.iter().filter_map(|part| part.as_ident());
    let skip = idents().count().saturating_sub(keep);
    let mut key = String::new();
    for (i, ident) in idents().skip(skip).enumerate() {
        if i > 0 {
            key.push('.');
        }
        if dialect == SqlDialect::Postgres && ident.quote_style.is_some() {
            key.push_str(&ident.value);
        } else if ident.value.is_ascii() {
            key.extend(ident.value.chars().map(|c| c.to_ascii_lowercase()));
        } else {
            key.push_str(&ident.value.to_lowercase());
        }
    }
    key
}

/// Table-level key of a name seen under `path`.
pub(super) fn schema_for_new(path: &[String]) -> &str {
    path.iter()
        .map(String::as_str)
        .find(|schema| !matches!(*schema, "$user" | "pg_catalog" | "pg_temp"))
        .unwrap_or("public")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(parts: &[(&str, bool)]) -> ObjectName {
        ObjectName::from(
            parts
                .iter()
                .map(|(value, quoted)| {
                    if *quoted {
                        Ident::with_quote('"', *value)
                    } else {
                        Ident::new(*value)
                    }
                })
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn name_key_matches_joining_the_folded_parts() {
        let names = [
            name(&[("Users", false)]),
            name(&[("Public", false), ("Users", false)]),
            name(&[("Public", true), ("Users", true)]),
            name(&[("db", false), ("Public", false), ("ABC \u{3a3}", false)]),
            name(&[("Mixed", true), ("\u{3a3}", false)]),
            name(&[]),
        ];
        for dialect in [SqlDialect::Postgres, SqlDialect::MySql, SqlDialect::Sqlite] {
            for name in &names {
                assert_eq!(
                    name_key(name, dialect),
                    key_of(&parts(name, dialect), dialect)
                );
            }
        }
    }

    #[test]
    fn lookup_key_finds_a_bare_name_along_the_path() {
        let mut map: FxHashMap<String, ()> = FxHashMap::default();
        map.insert("app.users".to_string(), ());
        let long = "x".repeat(200);
        map.insert(format!("app.{long}"), ());
        let path = vec!["public".to_string(), "app".to_string()];
        assert_eq!(lookup_key(&map, &path, "users"), Some("app.users"));
        let long_key = format!("app.{long}");
        assert_eq!(lookup_key(&map, &path, &long), Some(long_key.as_str()));
        assert_eq!(lookup_key(&map, &path, "missing"), None);
    }
}
