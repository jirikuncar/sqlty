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
//! and sqlc's `sqlc.embed(t)` to the row of `t`, padded with spaces so a diagnostic's span
//! stays where it is in the file.

use std::ops::Range;
use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;
use sqlty_sql::analyzer::{database_name, parse, route_named, statement_ranges, unknown_named};

use crate::checker::{star_finding, unparsed_message, Diagnostic};
use crate::options::Options;
use crate::rules::{Rule, RuleConfig, SQL_RULES};

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
    /// Each sqlc `sqlc.embed(t)`, rewritten to `t.*`: `(range of t.* in sql, range of t in
    /// the body)`.
    pub embeds: Vec<(Range<usize>, Range<usize>)>,
}

/// The rewritten SQL of a named query, its aiosql renames and its sqlc embeds.
pub type Rewritten = (
    String,
    Vec<(String, String)>,
    Vec<(Range<usize>, Range<usize>)>,
);

/// `-- name: <name><suffix>`, aiosql's header and sqlc's, told apart by sqlc's `:kind`.
static NAME_HEADER: LazyLock<Regex> =
    LazyLock::new(|| sqlty_sql::regex(r"(?m)^[ \t]*--[ \t]*name:[ \t]*([A-Za-z_][\w-]*)([^\n]*)$"));

/// The placeholders sqlc and aiosql read and the engine does not; strings, comments, casts
/// and `@@variables` match first so nothing inside them is rewritten.
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    sqlty_sql::regex(
        r#"'(?:[^']|'')*'|"(?:[^"]|"")*"|--[^\n]*|/\*(?s:.)*?\*/|::\w+|@@\w+|\bsqlc\.n?arg\(\s*'?(?P<arg>[A-Za-z_]\w*)'?\s*\)|\bsqlc\.embed\(\s*(?P<embed>[A-Za-z_]\w*)\s*\)|@(?P<at>[A-Za-z_]\w*)|:(?P<obj>[A-Za-z_]\w*)\.(?P<attr>[A-Za-z_]\w*)"#,
    )
});

/// `-- :name <name> [:kind]`, PugSQL's header.
static PUGSQL_HEADER: LazyLock<Regex> =
    LazyLock::new(|| sqlty_sql::regex(r"(?m)^[ \t]*--[ \t]*:name[ \t]+([A-Za-z_]\w*)[^\n]*$"));

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
                embeds: Vec::new(),
            }]
        }
    };
    headers
        .iter()
        .enumerate()
        .map(|(i, (header, name))| {
            let end = headers.get(i + 1).map_or(source.len(), |next| next.0.start);
            let body = (header.end + 1).min(end)..end;
            let (sql, renames, embeds) = placeholders(&source[body.clone()], format);
            NamedQuery {
                name: name.clone(),
                body,
                sql,
                renames,
                embeds,
            }
        })
        .collect()
}

/// `sql` with sqlc's or aiosql's placeholders rewritten to `:name` and `sqlc.embed(t)` to
/// `t.*`, each padded to its length, with the aiosql renames and the embeds.
///
/// ```
/// use sqlty_python::queries::{placeholders, Format};
///
/// let (sql, ..) = placeholders("id = sqlc.arg(id) AND name = @name", Format::Sqlc);
/// assert_eq!(sql, "id = :id          AND name = :name");
/// let (sql, renames, _) = placeholders("email = :user.email", Format::Aiosql);
/// assert_eq!((sql.as_str(), renames), ("email = :user_email", vec![("user_email".into(), "user.email".into())]));
/// ```
pub fn placeholders(sql: &str, format: Format) -> Rewritten {
    let mut out = String::with_capacity(sql.len());
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut embeds = Vec::new();
    let mut last = 0;
    for found in PLACEHOLDER.captures_iter(sql) {
        let whole = found.get(0).map_or(0..0, |m| m.range());
        let (prefix, name) = match (
            format,
            found.name("arg").or(found.name("at")),
            found.name("obj"),
            found.name("attr"),
        ) {
            (Format::Sqlc, Some(name), _, _) => (":", name.as_str().to_string()),
            (Format::Sqlc, None, None, None) => match found.name("embed") {
                Some(table) => {
                    let start = out.len() + whole.start - last;
                    embeds.push((start..start + table.len() + 2, table.range()));
                    ("", format!("{}.*", table.as_str()))
                }
                None => continue,
            },
            (Format::Aiosql, None, Some(obj), Some(attr)) => {
                let name = format!("{}_{}", obj.as_str(), attr.as_str());
                let original = format!("{}.{}", obj.as_str(), attr.as_str());
                if !renames.iter().any(|(renamed, _)| *renamed == name) {
                    renames.push((name.clone(), original));
                }
                (":", name)
            }
            _ => continue,
        };
        out.push_str(&sql[last..whole.start]);
        out.push_str(prefix);
        out.push_str(&name);
        out.extend(std::iter::repeat_n(
            ' ',
            whole.len() - name.len() - prefix.len(),
        ));
        last = whole.end;
    }
    out.push_str(&sql[last..]);
    (out, renames, embeds)
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
    let names = rules.enabled().intersects(SQL_RULES);
    for query in split(source, format, stem) {
        let body = query.sql.as_str();
        for statement in statement_ranges(body, dialect) {
            let at = query.body.start + statement.start..query.body.start + statement.end;
            let parsed = parse(&body[statement.clone()], dialect);
            if let Some(error) = parsed.error() {
                report(Rule::UnparsedQuery, unparsed_message(&error), at);
                continue;
            }
            let embeds: Vec<_> = query
                .embeds
                .iter()
                .filter(|(star, _)| statement.contains(&star.start))
                .collect();
            // An embed is sqlc's struct of the table, not a star to expand.
            let stars = if embeds.is_empty() {
                parsed.result_stars()
            } else {
                let mut text = body[statement.clone()].to_string();
                for (star, _) in &embeds {
                    let at = star.start - statement.start..star.end - statement.start;
                    text.replace_range(at.clone(), &format!("{:<1$}", "1", at.len()));
                }
                parse(&text, dialect).result_stars()
            };
            if let Some((rule, message)) =
                star_finding(stars, &body[statement.clone()], schema.is_some())
            {
                report(rule, message, at.clone());
            }
            if !names {
                continue;
            }
            for diagnostic in parsed.analyze_with_diagnostics(schema).1 {
                let mut message = diagnostic.message;
                let embed = embeds
                    .iter()
                    .find(|(star, _)| message.contains(&format!("`{}`", &body[star.clone()])));
                let span = match (embed, diagnostic.span) {
                    (Some((star, table)), _) => {
                        let written =
                            &source[query.body.start + table.start..query.body.start + table.end];
                        message = message.replace(
                            &format!("`{}`", &body[star.clone()]),
                            &format!("`sqlc.embed({written})`"),
                        );
                        query.body.start + table.start..query.body.start + table.end
                    }
                    (None, Some(span)) => at.start + span.start..at.start + span.end,
                    (None, None) => at.clone(),
                };
                report(diagnostic.rule, message, span);
            }
        }
    }
    found.sort_by_key(|diagnostic| diagnostic.span.start);
    found
}
