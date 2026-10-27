//! `SELECT *` expansion into an explicit column list, done on the query text.

use std::sync::LazyLock;

use regex::Regex;
use sqlparser::ast::{Query, SelectItem, SetExpr, Statement, TableFactor};

use super::dialect::SqlDialect;
use super::names::object_name;
use super::preprocess::parse_first;
use super::query::{leftmost_select, Analyzer};
use super::scope::{relation_name, relations, table_scope, Scope};
use super::{SchemaColumn, SchemaData};

pub(super) static STAR_ITEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(\w+)\s*\.\s*)?\*$").unwrap());

/// Expand SELECT * into an explicit `col::TYPE` list, keeping other projections verbatim.
pub fn expand_star(sql: &str, dialect: &str, schema: Option<&SchemaData>) -> Option<String> {
    let schema = schema.filter(|schema| !schema.is_empty())?;
    let dialect = SqlDialect::from_name(dialect);
    let stmt = parse_first(sql, dialect)?;
    let analyzer = Analyzer::new(schema, dialect);
    let Statement::Query(query) = &stmt else {
        return None;
    };
    let select = leftmost_select(&query.body)?;
    let sources: Vec<&TableFactor> = relations(&select.from).collect();
    let ctes: Vec<&str> = query
        .with
        .iter()
        .flat_map(|with| &with.cte_tables)
        .map(|cte| cte.alias.name.value.as_str())
        .collect();
    let scope: Scope = sources
        .iter()
        .flat_map(|factor| table_scope(factor, dialect))
        .collect();

    let upper = sql.to_ascii_uppercase();
    let select_idx = top_level(&upper, "SELECT", 0)?;
    let from_idx = top_level(&upper, "FROM", select_idx)?;

    let mut items = Vec::new();
    let mut has_star = false;
    for item in split_top_level(&sql[select_idx + "SELECT".len()..from_idx]) {
        let Some(star) = STAR_ITEM.captures(item) else {
            items.push(item.to_string());
            continue;
        };
        let reference = star.get(1).map(|m| m.as_str());
        // A bare `*` over several sources lists each one's columns qualified by it, in
        // FROM order: a bare `col` could be ambiguous.
        if reference.is_none() && sources.len() > 1 {
            if !analyzer.star_resolvable(&select.from, None) {
                return None;
            }
            for factor in &sources {
                let qualifier = unquoted_reference(factor)?;
                if ctes.iter().any(|cte| cte.eq_ignore_ascii_case(&qualifier)) {
                    return None;
                }
                let columns = analyzer.schema_star_columns(Some(&qualifier), &scope)?;
                items.extend(
                    columns
                        .iter()
                        .map(|col| cast_column(&qualifier, col, dialect)),
                );
            }
            has_star = true;
            continue;
        }
        let shadowed_by_cte = sources.iter().any(|factor| {
            let TableFactor::Table { name, .. } = factor else {
                return false;
            };
            reference.is_none_or(|reference| {
                relation_name(factor).is_some_and(|read| read.eq_ignore_ascii_case(reference))
            }) && ctes
                .iter()
                .any(|cte| cte.eq_ignore_ascii_case(&object_name(name)))
        });
        if shadowed_by_cte || !analyzer.star_resolvable(&select.from, reference) {
            return None;
        }
        let columns = analyzer.schema_star_columns(reference, &scope)?;
        let qualifier = reference.unwrap_or_default();
        items.extend(
            columns
                .iter()
                .map(|col| cast_column(qualifier, col, dialect)),
        );
        has_star = true;
    }
    has_star.then(|| {
        format!(
            "{}SELECT {} {}",
            &sql[..select_idx],
            items.join(", "),
            &sql[from_idx..]
        )
    })
}

/// `qualifier.col::TYPE` (just `col` without a qualifier); without `::` (MySQL, SQLite,
/// ...) the schema alone types the column, so it is written bare.
fn cast_column(qualifier: &str, col: &SchemaColumn, dialect: SqlDialect) -> String {
    let name = if qualifier.is_empty() {
        col.ident.clone()
    } else {
        format!("{qualifier}.{}", col.ident)
    };
    if dialect.has_cast_operator() && !col.sql_type.is_empty() {
        format!("{name}::{}", col.sql_type)
    } else {
        name
    }
}

/// The name a FROM item is referred to by, when it needs no quoting: its alias, else its
/// table name without the schema.
fn unquoted_reference(factor: &TableFactor) -> Option<String> {
    let ident = match factor {
        TableFactor::Table {
            alias: Some(alias), ..
        } => &alias.name,
        TableFactor::Table { name, .. } => name.0.last()?.as_ident()?,
        _ => return None,
    };
    ident.quote_style.is_none().then(|| ident.value.clone())
}

/// The stars that shape a statement's result, each named by its clause: `SELECT *`,
/// `SELECT u.*`, `RETURNING *`, and `WITH c AS (SELECT *)` for a CTE. `None` if the
/// statement does not parse. A star that doesn't shape the result, such as
/// `EXISTS (SELECT * ...)` or `count(*)`, is not listed.
///
/// Example:
/// ```
/// use sqlty_sql::analyzer::result_stars;
///
/// let stars = result_stars("SELECT u.*, o.id FROM users u JOIN orgs o ON o.id = u.org", "postgres");
/// assert_eq!(stars, Some(vec!["SELECT u.*".to_string()]));
/// let stars = result_stars("WITH c AS (SELECT * FROM t) DELETE FROM t RETURNING *", "postgres");
/// assert_eq!(stars, Some(vec!["WITH c AS (SELECT *)".to_string(), "RETURNING *".to_string()]));
/// assert_eq!(result_stars("SELECT 1 WHERE EXISTS (SELECT * FROM t)", "postgres"), Some(vec![]));
/// ```
pub fn result_stars(sql: &str, dialect: &str) -> Option<Vec<String>> {
    super::api::parse(sql, dialect).result_stars()
}

/// `clause` followed by the stars among `items` (`SELECT u.*, o.*`), if there are any.
fn wildcards(items: &[SelectItem], clause: &str) -> Option<String> {
    let found: Vec<String> = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(..)
            )
        })
        .map(ToString::to_string)
        .collect();
    (!found.is_empty()).then(|| format!("{clause} {}", found.join(", ")))
}

pub(super) fn statement_stars(stmt: &Statement, stars: &mut Vec<String>) {
    let returning = match stmt {
        Statement::Query(query) => return query_stars(query, stars),
        Statement::Insert(insert) => &insert.returning,
        Statement::Update(update) => &update.returning,
        Statement::Delete(delete) => &delete.returning,
        _ => return,
    };
    stars.extend(
        returning
            .as_deref()
            .and_then(|items| wildcards(items, "RETURNING")),
    );
}

fn query_stars(query: &Query, stars: &mut Vec<String>) {
    for cte in query.with.iter().flat_map(|with| &with.cte_tables) {
        let mut inner = Vec::new();
        query_stars(&cte.query, &mut inner);
        stars.extend(
            inner
                .into_iter()
                .map(|star| format!("WITH {} AS ({star})", cte.alias.name)),
        );
    }
    body_stars(&query.body, stars);
}

fn body_stars(body: &SetExpr, stars: &mut Vec<String>) {
    match body {
        SetExpr::Select(select) => stars.extend(wildcards(&select.projection, "SELECT")),
        SetExpr::Query(query) => query_stars(query, stars),
        SetExpr::SetOperation { left, right, .. } => {
            body_stars(left, stars);
            body_stars(right, stars);
        }
        SetExpr::Insert(stmt) | SetExpr::Update(stmt) | SetExpr::Delete(stmt) => {
            statement_stars(stmt, stars);
        }
        _ => {}
    }
}

pub(super) fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || !byte.is_ascii()
}

/// Byte index of `keyword` outside parentheses at or after `start`.
pub(super) fn top_level(sql_upper: &str, keyword: &str, start: usize) -> Option<usize> {
    let bytes = sql_upper.as_bytes();
    let mut depth = 0i32;
    for i in 0..bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        let end = i + keyword.len();
        if i >= start
            && depth == 0
            && sql_upper.as_bytes().get(i..end) == Some(keyword.as_bytes())
            && (i == 0 || !is_word(bytes[i - 1]))
            && (end == bytes.len() || !is_word(bytes[end]))
        {
            return Some(i);
        }
    }
    None
}

/// Split on commas outside parentheses, trimming each item.
pub(super) fn split_top_level(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, byte) in text.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' if depth == 0 => {
                items.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(text[start..].trim());
    items
}
