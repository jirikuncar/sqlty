//! Client-side query placeholders, rewritten to the numbered `$N` form sqlparser reads.
//!
//! Drivers disagree on how a query takes its arguments:
//!
//! | Style | Spelling | Used by |
//! |-------|----------|---------|
//! | format | `%s`, `%b`, `%t` | psycopg, MySQL drivers |
//! | pyformat | `%(name)s` | psycopg, MySQL drivers |
//! | numeric | `$1` | asyncpg, PostgreSQL |
//! | qmark | `?`, `?NNN` | sqlite3, MySQL |
//! | named | `:name` | sqlite3, SQLAlchemy's `text()` |
//!
//! [`rewrite`] turns the first four into numbered placeholders (`$N` in PostgreSQL, else
//! `?N`; a `$N` written in the query is left as it is) and reports the name of each parameter, so the analyzer reads every style the same way.
//! Quoted strings, quoted identifiers and comments are skipped, so a `?` or `:name` there
//! is text, and `%%` becomes `%`.

use std::collections::HashMap;

/// Which of the ambiguous spellings are placeholders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Styles {
    /// `?` and `?NNN`. In PostgreSQL `?` is also a JSONB operator.
    pub question_marks: bool,
    /// `:name`.
    pub colon_names: bool,
    /// Respell as `$N`, which only PostgreSQL's parser reads as a placeholder; else as `?N`,
    /// which every dialect's does.
    pub dollar_numbers: bool,
    /// A backslash escapes the next character in a string (MySQL). `E'...'` strings do
    /// in every dialect.
    pub backslash_escapes: bool,
}

impl Styles {
    fn number(self, number: usize) -> String {
        let sigil = if self.dollar_numbers { '$' } else { '?' };
        format!("{sigil}{number}")
    }
}

/// A query with its placeholders numbered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rewritten {
    /// The query with `%s`, `%(name)s`, `?` and `:name` respelled `$N` (or `?N`, see
    /// [`Styles::dollar_numbers`]).
    pub text: String,
    /// Name of `$1`, `$2`, ... as far as the rewrite numbered them: `None` for a positional
    /// placeholder. `$N` written in the query are not listed.
    pub names: Vec<Option<String>>,
}

/// `sql` with its placeholders respelled `$N`, numbered in order of appearance; a name
/// used again keeps its number.
///
/// # Examples
///
/// ```
/// use sqlty_sql::placeholders::{rewrite, Styles};
///
/// let styles = Styles { dollar_numbers: true, ..Styles::default() };
/// let found = rewrite("SELECT %(id)s, %s, %(id)s", styles);
/// assert_eq!(found.text, "SELECT $1, $2, $1");
/// assert_eq!(found.names, [Some("id".to_string()), None]);
/// ```
pub fn rewrite(sql: &str, styles: Styles) -> Rewritten {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len() + 8);
    let mut names: Vec<Option<String>> = Vec::new();
    let mut named: HashMap<String, usize> = HashMap::new();
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        let skip_to = match bytes[i] {
            b'\'' => Some(quoted_end(
                bytes,
                i,
                styles.backslash_escapes || escape_string(bytes, i),
            )),
            b'"' => Some(quoted_end(bytes, i, styles.backslash_escapes)),
            b'`' => Some(quoted_end(bytes, i, false)),
            b'-' if bytes.get(i + 1) == Some(&b'-') => Some(line_end(bytes, i)),
            b'/' if bytes.get(i + 1) == Some(&b'*') => Some(block_comment_end(bytes, i)),
            _ => None,
        };
        if let Some(end) = skip_to {
            i = end;
            continue;
        }
        let found = match bytes[i] {
            b'%' => percent(sql, i),
            b'?' if styles.question_marks && !matches!(bytes.get(i + 1), Some(b'|' | b'&')) => {
                question_mark(sql, i)
            }
            b':' if styles.colon_names => colon_name(sql, i),
            _ => None,
        };
        let Some((end, token)) = found else {
            i += 1;
            continue;
        };
        out.push_str(&sql[copied..i]);
        match token {
            Token::Percent => out.push('%'),
            Token::Positional => {
                names.push(None);
                out.push_str(&styles.number(names.len()));
            }
            Token::Numbered(number) => out.push_str(&styles.number(number)),
            Token::Named(name) => {
                let number = *named.entry(name.to_string()).or_insert_with(|| {
                    names.push(Some(name.to_string()));
                    names.len()
                });
                out.push_str(&styles.number(number));
            }
        }
        copied = end;
        i = end;
    }
    out.push_str(&sql[copied..]);
    Rewritten { text: out, names }
}

enum Token<'a> {
    /// `%%`, an escaped `%`.
    Percent,
    Positional,
    /// `?NNN`.
    Numbered(usize),
    Named(&'a str),
}

/// `%%`, `%s`, `%b`, `%t` and `%(name)s` at `at`; the index past it and what it is.
fn percent(sql: &str, at: usize) -> Option<(usize, Token<'_>)> {
    let rest = &sql[at + 1..];
    match rest.as_bytes().first()? {
        b'%' => Some((at + 2, Token::Percent)),
        b's' | b'b' | b't' => Some((at + 2, Token::Positional)),
        b'(' => {
            let close = rest.find(')')?;
            let name = &rest[1..close];
            matches!(rest.as_bytes().get(close + 1), Some(b's' | b'b' | b't'))
                .then_some(())
                .filter(|()| is_name(name))?;
            Some((at + 1 + close + 2, Token::Named(name)))
        }
        _ => None,
    }
}

/// `?` or `?NNN` at `at`.
fn question_mark(sql: &str, at: usize) -> Option<(usize, Token<'_>)> {
    let digits = sql[at + 1..].bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return Some((at + 1, Token::Positional));
    }
    let number = sql[at + 1..at + 1 + digits]
        .parse()
        .ok()
        .filter(|n| *n > 0)?;
    Some((at + 1 + digits, Token::Numbered(number)))
}

/// `:name` at `at`, as SQLAlchemy's `text()` reads one: not after a word, `:` or `\\`, and
/// not followed by `:`, so `::` casts, `a[1:2]` and `:a::int` are not placeholders.
fn colon_name(sql: &str, at: usize) -> Option<(usize, Token<'_>)> {
    let bytes = sql.as_bytes();
    if at > 0
        && (matches!(bytes[at - 1], b':' | b'_' | b'$' | b'\\')
            || bytes[at - 1].is_ascii_alphanumeric())
    {
        return None;
    }
    let first = *bytes.get(at + 1)?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let len = sql[at + 1..]
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    if bytes.get(at + 1 + len) == Some(&b':') {
        return None;
    }
    Some((at + 1 + len, Token::Named(&sql[at + 1..at + 1 + len])))
}

fn is_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Whether the quote at `at` opens an `E'...'` string.
fn escape_string(bytes: &[u8], at: usize) -> bool {
    at > 0
        && matches!(bytes[at - 1], b'E' | b'e')
        && (at == 1 || !(bytes[at - 2].is_ascii_alphanumeric() || bytes[at - 2] == b'_'))
}

/// Index just past the quoted string or identifier opening at `start`; a doubled quote
/// stays inside it.
fn quoted_end(bytes: &[u8], start: usize, backslashes: bool) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        if backslashes && bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == quote {
            if bytes.get(i + 1) != Some(&quote) {
                return i + 1;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// Index of the line break ending the comment at `start`.
fn line_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|b| *b == b'\n')
        .map_or(bytes.len(), |n| start + n)
}

/// Index just past the `/* */` comment opening at `start`.
fn block_comment_end(bytes: &[u8], start: usize) -> usize {
    bytes[start + 2..]
        .windows(2)
        .position(|w| w == b"*/")
        .map_or(bytes.len(), |n| start + 2 + n + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Styles = Styles {
        question_marks: true,
        colon_names: true,
        dollar_numbers: true,
        backslash_escapes: false,
    };
    const PLAIN: Styles = Styles {
        question_marks: false,
        colon_names: false,
        dollar_numbers: true,
        backslash_escapes: false,
    };

    fn text(sql: &str, styles: Styles) -> String {
        rewrite(sql, styles).text
    }

    #[test]
    fn psycopg_placeholders() {
        let found = rewrite("SELECT %s, %(a)s, %b, %(a)t, 5 %% 2, 5 % 2", PLAIN);
        assert_eq!(found.text, "SELECT $1, $2, $3, $2, 5 % 2, 5 % 2");
        assert_eq!(found.names, [None, Some("a".into()), None]);
    }

    #[test]
    fn native_placeholders_stay() {
        let found = rewrite("SELECT $2, $1", ALL);
        assert_eq!(found.text, "SELECT $2, $1");
        assert!(found.names.is_empty());
    }

    #[test]
    fn question_marks() {
        let numbered = Styles {
            dollar_numbers: false,
            ..ALL
        };
        assert_eq!(text("a = ? AND b = %s", numbered), "a = ?1 AND b = ?2");
        assert_eq!(text("a = ? AND b = ?2", ALL), "a = $1 AND b = $2");
        assert_eq!(text("a ? 'k'", PLAIN), "a ? 'k'");
        assert_eq!(text("a ?| b, c ?& d", ALL), "a ?| b, c ?& d");
    }

    #[test]
    fn colon_names() {
        let found = rewrite("x = :id AND y::int = :id OR z = :other", ALL);
        assert_eq!(found.text, "x = $1 AND y::int = $1 OR z = $2");
        assert_eq!(found.names, [Some("id".into()), Some("other".into())]);
        assert_eq!(text("a[1:2], t:x, x[y:z]", ALL), "a[1:2], t:x, x[y:z]");
        assert_eq!(text(":a::int, \\:b, :c", ALL), ":a::int, \\:b, $1");
        assert_eq!(text("x = :id", PLAIN), "x = :id");
    }

    #[test]
    fn literals_and_comments_are_text() {
        let sql = "SELECT '%s ? :a', \"?\", `:b` -- ? %s\n, /* ? */ ?";
        assert_eq!(
            text(sql, ALL),
            "SELECT '%s ? :a', \"?\", `:b` -- ? %s\n, /* ? */ $1"
        );
        assert_eq!(text("SELECT 'it''s ?', ?", ALL), "SELECT 'it''s ?', $1");
    }

    #[test]
    fn backslash_escapes() {
        let mysql = Styles {
            backslash_escapes: true,
            ..ALL
        };
        assert_eq!(text(r"SELECT 'it\'s ?', ?", mysql), r"SELECT 'it\'s ?', $1");
        assert_eq!(text(r"SELECT 'a\\', ?", mysql), r"SELECT 'a\\', $1");
        // Standard strings end at the quote; `E` strings take the backslash.
        assert_eq!(text(r"SELECT 'a\', ?", ALL), r"SELECT 'a\', $1");
        assert_eq!(text(r"SELECT E'it\'s ?', ?", ALL), r"SELECT E'it\'s ?', $1");
        assert_eq!(text(r"SELECT bare'x', ?", ALL), r"SELECT bare'x', $1");
    }

    #[test]
    fn stray_percent_is_text() {
        assert_eq!(
            text("a % b, %x, %(a b)s, %(", ALL),
            "a % b, %x, %(a b)s, %("
        );
    }
}
