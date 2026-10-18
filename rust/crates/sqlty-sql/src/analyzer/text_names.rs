//! MySQL and SQLite name an unaliased result column by the text of its expression.

use sqlparser::ast::{Expr, Value};
use sqlparser::tokenizer::{Location, Token, Tokenizer};

use super::dialect::SqlDialect;
use super::names::ident_name;

/// The name MySQL or SQLite gives an unaliased `expr` whose source text is `text`: a
/// column keeps its name (as written in MySQL), a MySQL string literal is its value, and
/// anything else is the text.
pub(super) fn text_column_name(expr: &Expr, text: &str, dialect: SqlDialect) -> String {
    let mut column = expr;
    while let Expr::Nested(inner) = column {
        column = inner;
    }
    let ident = match column {
        Expr::Identifier(ident) => Some(ident),
        Expr::CompoundIdentifier(parts) => parts.last(),
        _ => None,
    };
    if let Some(ident) = ident {
        return match dialect {
            SqlDialect::MySql => ident.value.clone(),
            _ => ident_name(ident),
        };
    }
    let literal = match expr {
        Expr::Prefixed { value, .. } => value,
        literal => literal,
    };
    if let (SqlDialect::MySql, Expr::Value(value)) = (dialect, literal) {
        if let Value::SingleQuotedString(s)
        | Value::DoubleQuotedString(s)
        | Value::NationalStringLiteral(s) = &value.value
        {
            return s.clone();
        }
    }
    text.to_string()
}

/// The source text of each item of the select list or `RETURNING` clause whose keyword
/// is the first token of `source[start..]`; `None` when the items cannot be told apart.
pub(super) fn item_texts(source: &str, start: usize, dialect: SqlDialect) -> Option<Vec<&str>> {
    let rest = source.get(start..)?;
    let parser_dialect = dialect.parser_dialect();
    let tokens = Tokenizer::new(parser_dialect.as_ref(), rest)
        .tokenize_with_location()
        .ok()?;
    let mut tokens = tokens
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .skip(1)
        .peekable();
    while tokens
        .next_if(|token| is_word(&token.token, MODIFIERS))
        .is_some()
    {}
    let lines = line_starts(rest);
    let mut items = Vec::new();
    let (mut first, mut last) = (None, None);
    let (mut depth, mut previous) = (0usize, None);
    for token in tokens {
        match &token.token {
            Token::LParen => depth += 1,
            Token::RParen if depth == 0 => break,
            Token::RParen => depth -= 1,
            Token::SemiColon if depth == 0 => break,
            Token::Comma if depth == 0 => {
                items.push(&rest[first.take()?..last.take()?]);
                continue;
            }
            word if depth == 0
                && is_word(word, CLAUSES)
                && !(is_word(word, &["FROM"]) && previous == Some("DISTINCT")) =>
            {
                break
            }
            _ => {}
        }
        previous = match &token.token {
            Token::Word(word) => Some(if word.value.eq_ignore_ascii_case("DISTINCT") {
                "DISTINCT"
            } else {
                ""
            }),
            _ => None,
        };
        first = first.or(offset(rest, &lines, token.span.start));
        last = Some(offset(rest, &lines, token.span.end)?);
    }
    items.push(&rest[first?..last?]);
    Some(items)
}

/// The byte offset of the `RETURNING` keyword outside parentheses, the last one if several.
pub(super) fn returning_start(source: &str, dialect: SqlDialect) -> Option<usize> {
    let parser_dialect = dialect.parser_dialect();
    let tokens = Tokenizer::new(parser_dialect.as_ref(), source)
        .tokenize_with_location()
        .ok()?;
    let lines = line_starts(source);
    let mut depth = 0usize;
    let mut found = None;
    for token in &tokens {
        match &token.token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            word if depth == 0 && is_word(word, &["RETURNING"]) => {
                found = offset(source, &lines, token.span.start);
            }
            _ => {}
        }
    }
    found
}

/// The byte offset of `location` in `source`, whose lines start at `lines`.
pub(super) fn offset(source: &str, lines: &[usize], location: Location) -> Option<usize> {
    let line_start = *lines.get(usize::try_from(location.line).ok()?.checked_sub(1)?)?;
    let column = usize::try_from(location.column).ok()?.checked_sub(1)?;
    let line = &source[line_start..];
    Some(
        line_start
            + line
                .char_indices()
                .nth(column)
                .map_or(line.len(), |(index, _)| index),
    )
}

pub(super) fn line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(source.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}

fn is_word(token: &Token, words: &[&str]) -> bool {
    matches!(token, Token::Word(word) if word.quote_style.is_none()
        && words.iter().any(|w| word.value.eq_ignore_ascii_case(w)))
}

/// Words between `SELECT` and its first item.
const MODIFIERS: &[&str] = &[
    "ALL",
    "DISTINCT",
    "DISTINCTROW",
    "HIGH_PRIORITY",
    "STRAIGHT_JOIN",
    "SQL_SMALL_RESULT",
    "SQL_BIG_RESULT",
    "SQL_BUFFER_RESULT",
    "SQL_NO_CACHE",
    "SQL_CALC_FOUND_ROWS",
];

/// Words that end a select list or `RETURNING` clause outside parentheses.
const CLAUSES: &[&str] = &[
    "FROM",
    "INTO",
    "WHERE",
    "GROUP",
    "HAVING",
    "WINDOW",
    "ORDER",
    "LIMIT",
    "UNION",
    "EXCEPT",
    "INTERSECT",
    "FOR",
    "LOCK",
    "OFFSET",
];
