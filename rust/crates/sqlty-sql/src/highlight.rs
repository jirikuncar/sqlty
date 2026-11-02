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
            let end = offset(sql, &line_starts(sql), error.location).unwrap_or(0);
            Tokenizer::new(dialect.as_ref(), &sql[..end])
                .tokenize_with_location()
                .unwrap_or_default()
        }
    };
    let lines = line_starts(sql);
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
        let (Some(kind), Some(start), Some(end)) = (
            kind,
            offset(sql, &lines, span.start),
            offset(sql, &lines, span.end),
        ) else {
            continue;
        };
        let end = start + sql[start..end].trim_end().len();
        if start < end && !percent.iter().any(|at| at.contains(&start)) {
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

fn line_starts(sql: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(sql.match_indices('\n').map(|(at, _)| at + 1))
        .collect()
}

/// The byte offset of a 1-based line and column (in characters).
fn offset(sql: &str, lines: &[usize], location: sqlparser::tokenizer::Location) -> Option<usize> {
    let start = *lines.get(usize::try_from(location.line).ok()?.checked_sub(1)?)?;
    let column = usize::try_from(location.column).ok()?.checked_sub(1)?;
    let line = &sql[start..];
    Some(
        start
            + line
                .char_indices()
                .nth(column)
                .map_or(line.len(), |(at, _)| at),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn colored<'a>(sql: &'a str, dialect: &str) -> Vec<(&'a str, Highlight)> {
        highlight(sql, dialect)
            .into_iter()
            .map(|(range, kind)| (&sql[range], kind))
            .collect()
    }

    #[test]
    fn keywords_are_sorted() {
        assert!(KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_vs_code_grammars_color_the_same_keywords() {
        for name in ["sql-injection", "sql-injection-multi"] {
            let path = format!(
                "{}/../../../editors/vscode/syntaxes/{name}.tmLanguage.json",
                env!("CARGO_MANIFEST_DIR")
            );
            let grammar = std::fs::read_to_string(&path).expect(&path);
            let alternatives = grammar
                .split_once("(?:ALL|")
                .and_then(|(_, rest)| rest.split_once(')'))
                .map(|(words, _)| {
                    std::iter::once("ALL")
                        .chain(words.split('|'))
                        .collect::<Vec<_>>()
                });
            assert_eq!(alternatives.as_deref(), Some(KEYWORDS), "{path}");
        }
    }

    #[test]
    fn names_that_sqlparser_knows_as_keywords_stay_names() {
        let sql = "SELECT name, status, \"select\", t.order FROM t WHERE type IS NOT NULL";
        let keywords: Vec<_> = colored(sql, "postgres")
            .into_iter()
            .filter(|(_, kind)| *kind == Highlight::Keyword)
            .map(|(text, _)| text)
            .collect();
        assert_eq!(keywords, ["SELECT", "FROM", "WHERE", "IS", "NOT", "NULL"]);
    }

    #[test]
    fn literals_comments_and_operators() {
        assert_eq!(
            colored(
                "SELECT 'a''b', 1.5, data->>'k', x::int /* c\n d */ FROM t;",
                "postgres"
            ),
            [
                ("SELECT", Highlight::Keyword),
                ("'a''b'", Highlight::String),
                ("1.5", Highlight::Number),
                ("->>", Highlight::Operator),
                ("'k'", Highlight::String),
                ("::", Highlight::Operator),
                ("/* c\n d */", Highlight::Comment),
                ("FROM", Highlight::Keyword),
            ]
        );
        assert_eq!(
            colored("SELECT %(id)s % 2, '%s' FROM t", "postgres"),
            [
                ("SELECT", Highlight::Keyword),
                ("%(id)s", Highlight::Parameter),
                ("%", Highlight::Operator),
                ("2", Highlight::Number),
                ("'%s'", Highlight::String),
                ("FROM", Highlight::Keyword),
            ]
        );
        assert_eq!(
            colored("SELECT ? -- é\n", "sqlite"),
            [
                ("SELECT", Highlight::Keyword),
                ("?", Highlight::Parameter),
                ("-- é", Highlight::Comment),
            ]
        );
    }

    #[test]
    fn an_unterminated_token_ends_the_colors() {
        assert_eq!(
            colored("SELECT 1 FROM t WHERE x = 'ab", "postgres"),
            [
                ("SELECT", Highlight::Keyword),
                ("1", Highlight::Number),
                ("FROM", Highlight::Keyword),
                ("WHERE", Highlight::Keyword),
                ("=", Highlight::Operator),
            ]
        );
    }
}
