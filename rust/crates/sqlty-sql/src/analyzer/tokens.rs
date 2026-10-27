//! The tokens of a query that [`super::input`] and [`super::respell`] edit: words,
//! parentheses, commas and other symbols, with a quoted string or name as one token and
//! comments skipped, so that no edit lands inside a literal or a comment.

use std::borrow::Cow;

use super::dialect::SqlDialect;
use super::preprocess::quoted_end;
use super::star_expand::is_word;

/// Replace `sql[start..end]` with the text.
pub(super) type Edit = (usize, usize, String);

/// `sql` with `edits` made, an edit that overlaps an earlier one dropped; borrowed when
/// there are none.
pub(super) fn apply(sql: &str, mut edits: Vec<Edit>) -> Cow<'_, str> {
    if edits.is_empty() {
        return Cow::Borrowed(sql);
    }
    edits.sort_by_key(|&(start, end, _)| (start, end));
    let (mut out, mut last) = (String::with_capacity(sql.len() + 32), 0);
    for (start, end, text) in edits {
        if start < last {
            continue;
        }
        out.push_str(&sql[last..start]);
        out.push_str(&text);
        last = end;
    }
    out.push_str(&sql[last..]);
    Cow::Owned(out)
}

/// The tokens directly inside the parenthesis that opens at token `open`.
pub(super) fn arguments(tokens: &[Token], open: usize) -> Vec<usize> {
    let mut depth = 0;
    let mut inside = Vec::new();
    for (j, token) in tokens.iter().enumerate().skip(open) {
        match token.kind {
            Kind::Open => depth += 1,
            Kind::Close if depth == 1 => break,
            Kind::Close => depth -= 1,
            _ if depth == 1 => inside.push(j),
            _ => {}
        }
    }
    inside
}

/// The token that closes the parenthesis opening at token `open`.
pub(super) fn closing(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0;
    tokens.iter().enumerate().skip(open).find_map(|(j, token)| {
        match token.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ => {}
        }
        (depth == 0).then_some(j)
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Word,
    Open,
    Close,
    Comma,
    Other,
}

pub(super) struct Token {
    pub(super) kind: Kind,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// The `$tag$` that opens a PostgreSQL dollar-quoted string at the start of `sql`.
pub(super) fn dollar_tag(sql: &str) -> Option<&str> {
    let rest = sql.strip_prefix('$')?;
    let end = rest.find('$')?;
    let tag = &rest[..end];
    let valid = tag
        .bytes()
        .enumerate()
        .all(|(k, b)| b == b'_' || b.is_ascii_alphabetic() || (k > 0 && b.is_ascii_digit()));
    valid.then(|| &sql[..end + 2])
}

/// The words, parentheses, commas and other tokens of `sql`, without whitespace and
/// comments; a quoted string or name is one `Other` token. A backslash escapes a quote in
/// MySQL's strings and in PostgreSQL's `E'...'`.
pub(super) fn tokens(sql: &str, dialect: SqlDialect) -> Vec<Token> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let kind = match bytes[i] {
            b if b.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i = sql[i..].find('\n').map_or(bytes.len(), |n| i + n);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = sql[i + 2..].find("*/").map_or(bytes.len(), |n| i + n + 4);
                continue;
            }
            b'\'' | b'"' if dialect == SqlDialect::MySql => {
                i = escaped_end(bytes, i);
                Kind::Other
            }
            b'\''
                if i > 0
                    && bytes[i - 1].eq_ignore_ascii_case(&b'e')
                    && (i == 1 || !is_word(bytes[i - 2])) =>
            {
                i = escaped_end(bytes, i);
                Kind::Other
            }
            b'\'' | b'"' | b'`' => {
                i = quoted_end(bytes, i);
                Kind::Other
            }
            b'$' if dollar_tag(&sql[i..]).is_some() => {
                let tag = dollar_tag(&sql[i..]).unwrap_or_default();
                let body = i + tag.len();
                i = sql[body..]
                    .find(tag)
                    .map_or(bytes.len(), |n| body + n + tag.len());
                Kind::Other
            }
            b'(' => {
                i += 1;
                Kind::Open
            }
            b')' => {
                i += 1;
                Kind::Close
            }
            b',' => {
                i += 1;
                Kind::Comma
            }
            b if is_word(b) => {
                while i < bytes.len() && is_word(bytes[i]) {
                    i += 1;
                }
                Kind::Word
            }
            _ => {
                i += sql[i..].chars().next().map_or(1, char::len_utf8);
                Kind::Other
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: i,
        });
    }
    tokens
}

/// The end of the string quoted at `start` in which a backslash escapes the next byte.
fn escaped_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote && bytes.get(i + 1) == Some(&quote) => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}
