//! `SELECT *` expansion into an explicit column list, done on the query text.

use std::sync::LazyLock;

use regex::Regex;
use sqlparser::ast::{Statement, TableFactor};

use super::dialect::SqlDialect;
use super::names::object_name;
use super::preprocess::parse_first;
use super::query::{leftmost_select, Analyzer};
use super::scope::{relations, table_scope};
use super::SchemaData;

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
    // Expansion drops table qualifiers, so with more than one source it could
    // write ambiguous columns.
    let [TableFactor::Table { name, .. }] = relations(&select.from).collect::<Vec<_>>()[..] else {
        return None;
    };
    if query.with.as_ref().is_some_and(|with| {
        with.cte_tables.iter().any(|cte| {
            cte.alias
                .name
                .value
                .eq_ignore_ascii_case(&object_name(name))
        })
    }) {
        return None;
    }
    let scope = table_scope(&select.from[0].relation, dialect);

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
        if !analyzer.star_resolvable(&select.from, reference) {
            return None;
        }
        let columns = analyzer.schema_star_columns(reference, &scope)?;
        items.extend(
            columns
                .iter()
                .map(|col| format!("{}::{}", col.ident, col.sql_type)),
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
