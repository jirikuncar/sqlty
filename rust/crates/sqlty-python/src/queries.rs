//! Named queries in `.sql` files: split by their headers and checked like the queries of
//! Python source.
//!
//! | Format | Header |
//! |--------|--------|
//! | `aiosql` | `-- name: get-user^` (`-` reads as `_`; the suffix is the result shape) |
//! | `sqlc` | `-- name: GetUser :one` |
//! | `pugsql` | `-- :name get_user :one` |
//! | `file` | none: the file is one query, named after it |
//!
//! `auto` picks the format of each file from its first header, and `file` when it has none.
//! A query runs from the line after its header to the next header; text before the first
//! header is no query.
//!
//! The engine reads `:name` and `$1`. sqlc's `@name`, `sqlc.arg(name)` and `sqlc.narg(name)`,
//! and aiosql's `:obj.attr`, are rewritten to `:name` and `:obj_attr` in [`NamedQuery::sql`],
//! padded with spaces so a diagnostic's span stays where it is in the file.

use std::ops::Range;
use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;
use sqlty_sql::analyzer::{database_name, parse, route_named, statement_ranges, unknown_named};
use sqlty_sql::diagnostics::NameRule;

use crate::checker::{star_finding, unparsed_message, Diagnostic};
use crate::options::Options;
use crate::rules::{Rule, RuleConfig};

/// How a `.sql` file names its queries (`query-format`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// The format of each file's first header; `File` when it has none.
    Auto,
    /// `-- name: get-user^`.
    Aiosql,
    /// `-- name: GetUser :one`.
    Sqlc,
    /// `-- :name get_user :one`.
    Pugsql,
    /// No header: one query, named after the file.
    File,
}

impl Format {
    /// The `query-format` values.
    pub const NAMES: [&'static str; 5] = ["auto", "aiosql", "sqlc", "pugsql", "file"];
}

impl FromStr for Format {
    type Err = ();

    fn from_str(name: &str) -> Result<Self, ()> {
        Ok(match name {
            "auto" => Format::Auto,
            "aiosql" => Format::Aiosql,
            "sqlc" => Format::Sqlc,
            "pugsql" => Format::Pugsql,
            "file" => Format::File,
            _ => return Err(()),
        })
    }
}

/// A query of a `.sql` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedQuery {
    /// The name its library calls it by: `get_user` for aiosql's `get-user^`.
    pub name: String,
    /// Byte range of its SQL in the file, from the line after its header.
    pub body: Range<usize>,
    /// Its SQL with the placeholders of its format rewritten to `:name`, as long as `body`.
    pub sql: String,
    /// `(name in sql, name in the file)` of each rewritten aiosql `:obj.attr`.
    pub renames: Vec<(String, String)>,
}

/// `-- name: <name><suffix>`, aiosql's header and sqlc's, told apart by sqlc's `:kind`.
static NAME_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*--[ \t]*name:[ \t]*([A-Za-z_][\w-]*)([^\n]*)$").expect("valid regex")
});

/// The placeholders sqlc and aiosql read and the engine does not; strings, comments, casts
/// and `@@variables` match first so nothing inside them is rewritten.
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"'(?:[^']|'')*'|"(?:[^"]|"")*"|--[^\n]*|/\*(?s:.)*?\*/|::\w+|@@\w+|\bsqlc\.n?arg\(\s*'?(?P<arg>[A-Za-z_]\w*)'?\s*\)|@(?P<at>[A-Za-z_]\w*)|:(?P<obj>[A-Za-z_]\w*)\.(?P<attr>[A-Za-z_]\w*)"#,
    )
    .expect("valid regex")
});

/// `-- :name <name> [:kind]`, PugSQL's header.
static PUGSQL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*--[ \t]*:name[ \t]+([A-Za-z_]\w*)[^\n]*$").expect("valid regex")
});

/// The named queries of `source`, a `.sql` file called `stem` (without its extension).
///
/// ```
/// use sqlty_python::queries::{split, Format};
///
/// let source = "-- name: get-user^\nSELECT 1;\n-- name: list-users\nSELECT 2;\n";
/// let names: Vec<_> = split(source, Format::Auto, "users").into_iter().map(|q| q.name).collect();
/// assert_eq!(names, ["get_user", "list_users"]);
/// ```
pub fn split(source: &str, format: Format, stem: &str) -> Vec<NamedQuery> {
    let format = match format {
        Format::Auto => detect(source),
        format => format,
    };
    let headers: Vec<(Range<usize>, String)> = match format {
        Format::Aiosql | Format::Sqlc => NAME_HEADER
            .captures_iter(source)
            .filter(|found| is_sqlc(&found[2]) == (format == Format::Sqlc))
            .map(|found| {
                let name = &found[1];
                let name = if format == Format::Aiosql {
                    name.replace('-', "_")
                } else {
                    name.to_string()
                };
                (found.get(0).map_or(0..0, |m| m.range()), name)
            })
            .collect(),
        Format::Pugsql => PUGSQL_HEADER
            .captures_iter(source)
            .map(|found| {
                (
                    found.get(0).map_or(0..0, |m| m.range()),
                    found[1].to_string(),
                )
            })
            .collect(),
        Format::File | Format::Auto => {
            return vec![NamedQuery {
                name: stem.to_string(),
                body: 0..source.len(),
                sql: source.to_string(),
                renames: Vec::new(),
            }]
        }
    };
    headers
        .iter()
        .enumerate()
        .map(|(i, (header, name))| {
            let end = headers.get(i + 1).map_or(source.len(), |next| next.0.start);
            let body = (header.end + 1).min(end)..end;
            let (sql, renames) = placeholders(&source[body.clone()], format);
            NamedQuery {
                name: name.clone(),
                body,
                sql,
                renames,
            }
        })
        .collect()
}

/// `sql` with sqlc's or aiosql's placeholders rewritten to `:name`, each padded to its length,
/// and the aiosql renames.
///
/// ```
/// use sqlty_python::queries::{placeholders, Format};
///
/// let (sql, _) = placeholders("id = sqlc.arg(id) AND name = @name", Format::Sqlc);
/// assert_eq!(sql, "id = :id          AND name = :name");
/// let (sql, renames) = placeholders("email = :user.email", Format::Aiosql);
/// assert_eq!((sql.as_str(), renames), ("email = :user_email", vec![("user_email".into(), "user.email".into())]));
/// ```
pub fn placeholders(sql: &str, format: Format) -> (String, Vec<(String, String)>) {
    let mut out = String::with_capacity(sql.len());
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut last = 0;
    for found in PLACEHOLDER.captures_iter(sql) {
        let whole = found.get(0).map_or(0..0, |m| m.range());
        let name = match (
            format,
            found.name("arg").or(found.name("at")),
            found.name("obj"),
            found.name("attr"),
        ) {
            (Format::Sqlc, Some(name), _, _) => name.as_str().to_string(),
            (Format::Aiosql, None, Some(obj), Some(attr)) => {
                let name = format!("{}_{}", obj.as_str(), attr.as_str());
                let original = format!("{}.{}", obj.as_str(), attr.as_str());
                if !renames.iter().any(|(renamed, _)| *renamed == name) {
                    renames.push((name.clone(), original));
                }
                name
            }
            _ => continue,
        };
        out.push_str(&sql[last..whole.start]);
        out.push(':');
        out.push_str(&name);
        out.extend(std::iter::repeat_n(' ', whole.len() - name.len() - 1));
        last = whole.end;
    }
    out.push_str(&sql[last..]);
    (out, renames)
}

/// The format of the first header of `source`; `file` when it has none.
fn detect(source: &str) -> Format {
    let name = NAME_HEADER.captures(source);
    let pugsql = PUGSQL_HEADER.find(source);
    match (name, pugsql) {
        (Some(found), pugsql)
            if pugsql.is_none_or(|p| found.get(0).is_some_and(|m| m.start() < p.start())) =>
        {
            if is_sqlc(&found[2]) {
                Format::Sqlc
            } else {
                Format::Aiosql
            }
        }
        (_, Some(_)) => Format::Pugsql,
        _ => Format::File,
    }
}

/// Whether what follows a `-- name:` header's name is sqlc's `:kind`.
fn is_sqlc(rest: &str) -> bool {
    rest.trim_start().starts_with(':')
}

/// Check each statement of each named query of `source`: `unparsed-query`, `select-star` and
/// the name rules, at the name when the analyzer knows where it is written, else at the
/// statement.
pub fn check_queries(
    source: &str,
    format: Format,
    stem: &str,
    options: &Options,
    rules: &RuleConfig,
) -> Vec<Diagnostic> {
    let database = database_name(source);
    let (dialect, schema) = match options.schema {
        Some(schema) => route_named(database, options.dialect.name(), schema),
        None => (options.dialect.name(), None),
    };
    let mut found = Vec::new();
    let mut report = |rule: Rule, message: String, span: Range<usize>| {
        if rules.is_enabled(rule) {
            found.push(Diagnostic {
                rule,
                message,
                severity: rules.severity(rule),
                line: source[..span.start].matches('\n').count() + 1,
                span,
                fix: Vec::new(),
            });
        }
    };
    if let Some(message) = database.and_then(|name| unknown_named(name, options.schema)) {
        let header = source.len() - source.trim_start().len();
        let end = source[header..]
            .find('\n')
            .map_or(source.len(), |at| header + at);
        report(
            Rule::UnknownDatabase,
            message,
            header..end.min(source.len()),
        );
    }
    let names = NameRule::ALL
        .iter()
        .any(|&rule| rules.is_enabled(Rule::Name(rule)));
    for query in split(source, format, stem) {
        let body = query.sql.as_str();
        for statement in statement_ranges(body, dialect) {
            let at = query.body.start + statement.start..query.body.start + statement.end;
            let parsed = parse(&body[statement.clone()], dialect);
            if let Some(error) = parsed.error() {
                report(Rule::UnparsedQuery, unparsed_message(&error), at);
                continue;
            }
            if let Some((rule, message)) = star_finding(
                parsed.result_stars(),
                &body[statement.clone()],
                schema.is_some(),
            ) {
                report(rule, message, at.clone());
            }
            if !names {
                continue;
            }
            for diagnostic in parsed.analyze_with_diagnostics(schema).1 {
                let span = diagnostic.span.map_or(at.clone(), |span| {
                    at.start + span.start..at.start + span.end
                });
                report(Rule::Name(diagnostic.rule), diagnostic.message, span);
            }
        }
    }
    found.sort_by_key(|diagnostic| diagnostic.span.start);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str, format: Format) -> Vec<(String, String)> {
        split(source, format, "stem")
            .into_iter()
            .map(|query| (query.name, source[query.body].trim().to_string()))
            .collect()
    }

    #[test]
    fn headers_of_each_format() {
        let pair = |name: &str, sql: &str| (name.to_string(), sql.to_string());
        let aiosql = "-- preamble\n-- name: get-user^\n-- doc\nSELECT 1;\n\n-- name: add!\nINSERT INTO t VALUES (1)";
        assert_eq!(
            names(aiosql, Format::Auto),
            [
                pair("get_user", "-- doc\nSELECT 1;"),
                pair("add", "INSERT INTO t VALUES (1)")
            ]
        );
        let sqlc = "-- name: GetUser :one\nSELECT 1;\n-- name: ListUsers :many\nSELECT 2;";
        assert_eq!(
            names(sqlc, Format::Auto),
            [pair("GetUser", "SELECT 1;"), pair("ListUsers", "SELECT 2;")]
        );
        let pugsql = "-- :name find_user :one\nSELECT 1";
        assert_eq!(names(pugsql, Format::Auto), [pair("find_user", "SELECT 1")]);
        assert_eq!(names("SELECT 1", Format::Auto), [pair("stem", "SELECT 1")]);
        assert_eq!(names(aiosql, Format::File)[0].0, "stem");
        assert!(
            names(sqlc, Format::Aiosql).is_empty(),
            "sqlc headers are not aiosql's"
        );
    }

    #[test]
    fn placeholders_keep_their_length_and_skip_strings_and_casts() {
        let sql = "SELECT '@a', :u.x, x::pg.t, @@v -- @b\nWHERE sqlc.narg( 'n' ) = @name";
        assert_eq!(
            placeholders(sql, Format::Sqlc).0,
            "SELECT '@a', :u.x, x::pg.t, @@v -- @b\nWHERE :n               = :name"
        );
        assert_eq!(
            placeholders(sql, Format::Aiosql).0,
            "SELECT '@a', :u_x, x::pg.t, @@v -- @b\nWHERE sqlc.narg( 'n' ) = @name"
        );
        assert_eq!(placeholders(sql, Format::Pugsql).0, sql);
    }

    #[test]
    fn a_header_on_the_last_line_has_an_empty_body() {
        let source = "SELECT 0;\n-- name: last";
        let [query] = split(source, Format::Aiosql, "stem").try_into().unwrap();
        assert!(source[query.body].is_empty());
    }
}
