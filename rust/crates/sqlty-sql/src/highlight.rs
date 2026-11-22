//! What to color in a query: keywords, literals, comments, operators and placeholders.
//!
//! Names stay uncolored. sqlparser knows 900 keywords, `name`, `status` and `type` among
//! them, so a word is a keyword only when it is in [`KEYWORDS`]: the words PostgreSQL
//! reserves (<https://www.postgresql.org/docs/17/sql-keywords-appendix.html>) and the
//! verbs and clauses of a statement, which a query rarely uses as a name. A quoted word,
//! or one after a `.`, is a name.
//!
//! ```
//! use sqlty_sql::highlight::{highlight, Highlight};
//!
//! let sql = "SELECT name FROM users WHERE id = $1 -- one";
//! let found: Vec<_> = highlight(sql, "postgres")
//!     .into_iter()
//!     .map(|(range, kind)| (&sql[range], kind))
//!     .collect();
//! assert_eq!(
//!     found,
//!     [
//!         ("SELECT", Highlight::Keyword),
//!         ("FROM", Highlight::Keyword),
//!         ("WHERE", Highlight::Keyword),
//!         ("=", Highlight::Operator),
//!         ("$1", Highlight::Parameter),
//!         ("-- one", Highlight::Comment),
//!     ]
//! );
//! ```

use std::ops::Range;

use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer, Whitespace};

use crate::analyzer::SqlDialect;
use crate::placeholders::percent_placeholders;

/// How to color a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Highlight {
    /// `SELECT`, `FROM`, `NULL`, ...
    Keyword,
    /// A quoted string.
    String,
    /// A number.
    Number,
    /// `-- ...` or `/* ... */`.
    Comment,
    /// `=`, `::`, `->>`, `*`, ...
    Operator,
    /// `$1`, `?`, `:name`, `%s`, `%(name)s`.
    Parameter,
}

/// The tokens of `sql` to color, in order, as byte ranges. A query that stops in the
/// middle of a token, as it does while typed, is colored up to that token.
pub fn highlight(sql: &str, dialect: &str) -> Vec<(Range<usize>, Highlight)> {
    let dialect = SqlDialect::from_name(dialect).parser_dialect();
    let tokens = match Tokenizer::new(dialect.as_ref(), sql).tokenize_with_location() {
        Ok(tokens) => tokens,
        Err(error) => {
            let end = Offsets::new(sql).offset(error.location).unwrap_or(0);
            Tokenizer::new(dialect.as_ref(), &sql[..end])
                .tokenize_with_location()
                .unwrap_or_default()
        }
    };
    let mut offsets = Offsets::new(sql);
    let percent = percent_placeholders(sql);
    let mut found: Vec<_> = percent
        .iter()
        .map(|at| (at.clone(), Highlight::Parameter))
        .collect();
    let mut after_period = false;
    for TokenWithSpan { token, span } in tokens {
        let kind = match &token {
            Token::Whitespace(
                Whitespace::SingleLineComment { .. } | Whitespace::MultiLineComment(_),
            ) => Some(Highlight::Comment),
            Token::Whitespace(_) => continue,
            Token::Word(word) => (word.quote_style.is_none()
                && !after_period
                && KEYWORDS
                    .binary_search(&word.value.to_ascii_uppercase().as_str())
                    .is_ok())
            .then_some(Highlight::Keyword),
            Token::Number(..) => Some(Highlight::Number),
            Token::Placeholder(_) | Token::Question => Some(Highlight::Parameter),
            Token::Comma
            | Token::LParen
            | Token::RParen
            | Token::LBracket
            | Token::RBracket
            | Token::LBrace
            | Token::RBrace
            | Token::Period
            | Token::SemiColon
            | Token::EOF => None,
            Token::Char(_) | Token::CustomBinaryOperator(_) => Some(Highlight::Operator),
            other if is_string(other) => Some(Highlight::String),
            _ => Some(Highlight::Operator),
        };
        after_period = token == Token::Period;
        let (Some(kind), Some(start), Some(end)) =
            (kind, offsets.offset(span.start), offsets.offset(span.end))
        else {
            continue;
        };
        let end = start + sql[start..end].trim_end().len();
        let after = percent.partition_point(|at| at.end <= start);
        if start < end && !percent.get(after).is_some_and(|at| at.contains(&start)) {
            found.push((start..end, kind));
        }
    }
    found.sort_by_key(|(range, _)| range.start);
    found
}

fn is_string(token: &Token) -> bool {
    matches!(
        token,
        Token::SingleQuotedString(_)
            | Token::DoubleQuotedString(_)
            | Token::TripleSingleQuotedString(_)
            | Token::TripleDoubleQuotedString(_)
            | Token::DollarQuotedString(_)
            | Token::SingleQuotedByteStringLiteral(_)
            | Token::DoubleQuotedByteStringLiteral(_)
            | Token::TripleSingleQuotedByteStringLiteral(_)
            | Token::TripleDoubleQuotedByteStringLiteral(_)
            | Token::SingleQuotedRawStringLiteral(_)
            | Token::DoubleQuotedRawStringLiteral(_)
            | Token::TripleSingleQuotedRawStringLiteral(_)
            | Token::TripleDoubleQuotedRawStringLiteral(_)
            | Token::NationalStringLiteral(_)
            | Token::QuoteDelimitedStringLiteral(_)
            | Token::NationalQuoteDelimitedStringLiteral(_)
            | Token::EscapedStringLiteral(_)
            | Token::UnicodeStringLiteral(_)
            | Token::HexStringLiteral(_)
    )
}

/// Byte offsets of 1-based lines and columns (in characters), read forward from the last
/// one asked for: tokens come in order, so a long line is walked once, not once per token.
struct Offsets<'s> {
    sql: &'s str,
    lines: Vec<usize>,
    line: usize,
    column: usize,
    at: usize,
}

impl<'s> Offsets<'s> {
    fn new(sql: &'s str) -> Self {
        let lines = std::iter::once(0)
            .chain(sql.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        Offsets {
            sql,
            lines,
            line: 0,
            column: 0,
            at: 0,
        }
    }

    fn offset(&mut self, location: sqlparser::tokenizer::Location) -> Option<usize> {
        let line = usize::try_from(location.line).ok()?.checked_sub(1)?;
        let column = usize::try_from(location.column).ok()?.checked_sub(1)?;
        let start = *self.lines.get(line)?;
        if line != self.line || column < self.column {
            (self.line, self.column, self.at) = (line, 0, start);
        }
        let Some((ahead, _)) = self.sql[self.at..].char_indices().nth(column - self.column) else {
            return Some(self.sql.len());
        };
        self.column = column;
        self.at += ahead;
        Some(self.at)
    }
}

/// Sorted, for a binary search.
const KEYWORDS: &[&str] = &[
    "ALL",
    "ALTER",
    "ANALYSE",
    "ANALYZE",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "ASYMMETRIC",
    "BETWEEN",
    "BOTH",
    "BY",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "CONFLICT",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DO",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "FALSE",
    "FETCH",
    "FILTER",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "GROUP",
    "HAVING",
    "ILIKE",
    "IN",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "ISNULL",
    "JOIN",
    "LATERAL",
    "LEADING",
    "LEFT",
    "LIKE",
    "LIMIT",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "MERGE",
    "NATURAL",
    "NOT",
    "NOTHING",
    "NOTNULL",
    "NULL",
    "NULLS",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PARTITION",
    "PRIMARY",
    "RECURSIVE",
    "REFERENCES",
    "REPLACE",
    "RETURNING",
    "RIGHT",
    "SELECT",
    "SESSION_USER",
    "SET",
    "SIMILAR",
    "SOME",
    "SYMMETRIC",
    "TABLE",
    "THEN",
    "TO",
    "TRAILING",
    "TRUE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "VARIADIC",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];
