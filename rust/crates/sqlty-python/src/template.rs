//! Reading a t-string as a query: a field that only passes a value becomes a `%s`
//! placeholder. psycopg's `:s`, `:b` and `:t` only choose the wire format; in a module
//! that uses psycopg, its `:l` literal is a value too, its `:i` identifier is read where
//! it can't change the result columns, and its `:q` snippet is inlined when it names a
//! template of the module.

use std::sync::LazyLock;

use regex::Regex;
use sqlty_sql::analyzer::{parse_error, returns_rows};

use crate::rewriter::marked_query;
use crate::tokens::{field_end, string_end, tokenize, Kind};

/// An import of psycopg or of sqlty's psycopg helpers.
static PSYCOPG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(?:(?:import|from)[ \t]+(?:sqlty\.drivers\.)?psycopg\b|from[ \t]+sqlty\.drivers[ \t]+import\b[^\n]*\bpsycopg\b)",
    )
    .unwrap()
});

/// What a `:i` field reads as, so the query still parses.
const IDENTIFIER: &str = "\"sqlty_identifier\"";

/// What a field that passes a value reads as.
const PLACEHOLDER: &str = "%s";

/// What a `:l` field reads as where a placeholder doesn't parse.
const LITERAL: &str = "'sqlty_literal'";

/// How deep `:q` snippets may nest, which also stops a template that names itself.
const NESTING: usize = 8;

/// The text of a t-string literal (prefix and quotes included) of the module `source` as
/// a query: each field that passes a value becomes a `%s` placeholder, `{{` and `}}`
/// become braces, and a `%` of the text is escaped as `%%`. `Err` holds the first field
/// that changes the SQL in a way sqlty can't follow, such as `{name!r}` or `{x:>10}`.
pub(crate) fn template_content(literal: &str, source: &str) -> Result<String, String> {
    let read = |value| read(literal, source, 0, value);
    let mut query = read(PLACEHOLDER)?;
    // A `:l` field also goes where a parameter can't, such as `NOTIFY`'s payload.
    if query.literal
        && marked_query(&query.sql).is_some_and(|sql| parse_error(&sql, "postgres").is_some())
    {
        query = read(LITERAL)?;
    }
    match query.identifier {
        Some(field)
            if marked_query(&query.sql).is_some_and(|sql| returns_rows(&sql, "postgres")) =>
        {
            Err(field)
        }
        _ => Ok(query.sql),
    }
}

/// A t-string read as a query.
struct Query {
    sql: String,
    /// The first `:i` field.
    identifier: Option<String>,
    /// Whether a `:l` field was read.
    literal: bool,
}

/// The query of `literal`, with `value` for each `:l` field.
fn read(literal: &str, source: &str, depth: usize, value: &str) -> Result<Query, String> {
    let quoted = literal.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let triple = quoted.starts_with("\"\"\"") || quoted.starts_with("'''");
    let quote = if triple { 3 } else { 1 };
    let body = quoted
        .get(quote..quoted.len().saturating_sub(quote))
        .ok_or_else(String::new)?;
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut identifier = None;
    let mut literal = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                let next = body[i + 1..].chars().next().map_or(0, char::len_utf8);
                out.push_str(&body[i..i + 1 + next]);
                i += 1 + next;
            }
            b'{' if bytes.get(i + 1) == Some(&b'{') => {
                out.push('{');
                i += 2;
            }
            b'}' if bytes.get(i + 1) == Some(&b'}') => {
                out.push('}');
                i += 2;
            }
            b'{' => {
                let end = field_end(bytes, i + 1, triple).ok_or_else(|| body[i..].to_string())?;
                let field = &body[i..end];
                let blocked = || field.to_string();
                let (expr, spec) = split_field(&body[i + 1..end - 1]).ok_or_else(blocked)?;
                let psycopg = || PSYCOPG.is_match(source);
                match spec {
                    "" | ":s" | ":b" | ":t" => out.push_str(PLACEHOLDER),
                    ":l" if psycopg() => {
                        out.push_str(value);
                        literal = true;
                    }
                    ":i" if psycopg() => {
                        out.push_str(IDENTIFIER);
                        identifier.get_or_insert_with(blocked);
                    }
                    ":q" if depth < NESTING && psycopg() => {
                        let snippet = module_template(expr.trim(), source).ok_or_else(blocked)?;
                        let inner =
                            read(snippet, source, depth + 1, value).map_err(|_| blocked())?;
                        out.push_str(&inner.sql);
                        if inner.identifier.is_some() {
                            identifier.get_or_insert_with(blocked);
                        }
                        literal |= inner.literal;
                    }
                    _ => return Err(blocked()),
                }
                i = end;
            }
            b'%' => {
                out.push_str("%%");
                i += 1;
            }
            _ => {
                let len = body[i..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&body[i..i + len]);
                i += len;
            }
        }
    }
    Ok(Query {
        sql: out,
        identifier,
        literal,
    })
}

/// A replacement field split into its expression and the rest: `""`, or a `:spec`.
/// `None` for a conversion (`!r`), a nested spec or a `{x=}` debug field.
fn split_field(field: &str) -> Option<(&str, &str)> {
    let bytes = field.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => i = string_end(bytes, i, false)?,
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'!' if depth == 0 && bytes.get(i + 1) != Some(&b'=') => return None,
            b':' if depth == 0 => {
                let spec = &field[i..];
                return (!field[..i].trim().is_empty() && !spec.contains('{'))
                    .then_some((&field[..i], spec));
            }
            b'=' if depth == 0 => {
                let rest = field[i + 1..].trim();
                let before = bytes.get(i.wrapping_sub(1)).copied();
                if rest.is_empty() && !matches!(before, Some(b'=' | b'!' | b'<' | b'>')) {
                    return None;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    (!field.trim().is_empty()).then_some((field, ""))
}

/// The t-string literal `name` is assigned at module level of `source`, when that is
/// the name's only module-level assignment.
fn module_template<'a>(name: &str, source: &'a str) -> Option<&'a str> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    let tokens = tokenize(source);
    let text = |i: usize| &source[tokens[i].span.clone()];
    let mut assignments = (0..tokens.len().saturating_sub(2)).filter(|&i| {
        tokens[i].kind == Kind::Name
            && tokens[i].depth == 0
            && (i == 0 || tokens[i - 1].kind == Kind::Newline)
            && text(i) == name
            && tokens[i + 1].kind == Kind::Op
            && text(i + 1) == "="
            && line_start(source, tokens[i].span.start)
    });
    let first = assignments.next()?;
    if assignments.next().is_some() {
        return None;
    }
    let value = first + 2;
    let ends = tokens
        .get(value + 1)
        .is_none_or(|token| matches!(token.kind, Kind::Newline | Kind::Comment));
    (tokens[value].kind == Kind::Str && tokens[value].prefix(source) == "t" && ends)
        .then(|| text(value))
}

/// Whether `at` starts its line: a module-level statement, not an indented one.
fn line_start(source: &str, at: usize) -> bool {
    at == 0 || source.as_bytes()[at - 1] == b'\n'
}
