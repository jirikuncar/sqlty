//! What sqlparser reads of a query: the `--sql` marker line dropped, psycopg placeholders
//! as `NULL`, and the syntax it rejects respelled. Every fix is an edit on one token stream
//! ([`super::tokens`]), so none lands inside a string literal, a quoted name or a comment.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use super::dialect::SqlDialect;
use super::respell::respell;
use super::tokens::{apply, closing, tokens, Edit, Kind, Token};

/// A leading `--sql` marker line: MySQL only reads `--` followed by whitespace as a comment.
static SQL_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\s*--sql[^\n]*\n?").unwrap());

/// PostgreSQL types sqlparser reads a `type 'string'` literal of only as a cast.
const POSTGRES_TYPED_STRINGS: [&str; 19] = [
    "bpchar",
    "cidr",
    "citext",
    "daterange",
    "hstore",
    "inet",
    "int4range",
    "int8range",
    "jsonpath",
    "macaddr",
    "macaddr8",
    "money",
    "name",
    "numrange",
    "oid",
    "pg_lsn",
    "tsrange",
    "tstzrange",
    "txid_snapshot",
];

/// `sql` as sqlparser should read it: the `--sql` marker line dropped (a leading space
/// form like `--sql SELECT 1` leaves nothing), psycopg placeholders (`%s`, `%(name)s`,
/// `%b`, `%t`) as `NULL`, a value of unknown type, and `%%` as `%`, and the syntax sqlparser
/// rejects respelled as the closest it reads:
///
/// - ISO's `BINARY LARGE OBJECT` as `BLOB` and `T MULTISET` as the array `T[]`;
/// - in PostgreSQL, the `WHERE` of `ON CONFLICT (...) WHERE ... DO` dropped (the partial
///   index that arbitrates the conflict does not change the result), `OVERRIDING SYSTEM
///   VALUE` and `OVERRIDING USER VALUE` dropped (they pick the stored value, not its
///   type), `FOR NO KEY UPDATE`
///   as `FOR UPDATE`, `FOR KEY SHARE` as `FOR SHARE`, `OF` keeping only its first table,
///   and `cidr '...'` and similar literals as casts;
/// - in PostgreSQL and MySQL, a `TABLE name` query as `SELECT * FROM name`;
/// - in SQLite, `INDEXED BY name` and `NOT INDEXED` dropped: they only pick an index;
/// - the rest of [`respell`].
///
/// A placeholder in a string literal is left alone, as the database would read it, but
/// `%%` there still becomes `%`, as psycopg sends it.
pub(super) fn parser_input(sql: &str, dialect: SqlDialect) -> Input<'_> {
    let marker = SQL_MARKER.find(sql).map_or(0, |marker| marker.end());
    let sql = &sql[marker..];
    let tokens = tokens(sql, dialect);
    let mut edits = Vec::new();
    fixes(sql, &tokens, dialect, &mut edits);
    placeholders(sql, &tokens, &mut edits);
    let mut respelling = Vec::new();
    respell(sql, &tokens, dialect, &mut respelling);
    let respelled = (!respelling.is_empty()).then(|| {
        respelling.extend(edits.iter().cloned());
        respelling
    });
    let parsed = respelled.as_ref().unwrap_or(&edits);
    let mut moves: Vec<_> = parsed
        .iter()
        .map(|(start, end, text)| (*start, *end, text.len()))
        .collect();
    moves.sort_unstable();
    let mut last = 0;
    moves.retain(|&(start, end, _)| {
        let kept = start >= last;
        if kept {
            last = end;
        }
        kept
    });
    Input {
        respelled: respelled.map(|respelling| apply(sql, respelling).into_owned()),
        source: apply(sql, edits),
        marker,
        moves,
    }
}

/// A query as sqlparser reads it, from [`parser_input`].
pub(super) struct Input<'a> {
    /// The query without [`respell`]'s edits, which MySQL and SQLite name columns from.
    pub(super) source: Cow<'a, str>,
    /// The query with them, when it has any.
    respelled: Option<String>,
    /// The length of the `--sql` line before the query.
    marker: usize,
    /// The edits that made [`Input::text`]: start, end and the length of the new text.
    moves: Vec<(usize, usize, usize)>,
}

impl Input<'_> {
    /// What sqlparser parses.
    pub(super) fn text(&self) -> &str {
        self.respelled.as_deref().unwrap_or(&self.source)
    }

    /// Where the byte at `offset` of [`Input::text`] was in the query; an offset inside an
    /// edit's text maps to the edit's start, or its end with `end`.
    pub(super) fn original(&self, offset: usize, end: bool) -> usize {
        let mut shift = 0isize;
        for &(start, stop, len) in &self.moves {
            let at = start.saturating_add_signed(shift);
            if offset < at || (end && offset == at) {
                break;
            }
            if offset < at + len || (end && offset == at + len) {
                return self.marker + if end { stop } else { start };
            }
            shift += len as isize - (stop - start) as isize;
        }
        self.marker + offset.saturating_add_signed(-shift)
    }

    pub(super) fn into_owned(self) -> Input<'static> {
        Input {
            source: Cow::Owned(self.source.into_owned()),
            respelled: self.respelled,
            marker: self.marker,
            moves: self.moves,
        }
    }
}

/// Schema DDL with ISO's `BINARY LARGE OBJECT` as `BLOB` and `T MULTISET` as `T[]`.
pub(super) fn iso_types(sql: &str, dialect: SqlDialect) -> Cow<'_, str> {
    let tokens = tokens(sql, dialect);
    let mut edits = Vec::new();
    for i in 0..tokens.len() {
        iso_type(sql, &tokens, i, &mut edits);
    }
    apply(sql, edits)
}

fn fixes(sql: &str, tokens: &[Token], dialect: SqlDialect, edits: &mut Vec<Edit>) {
    let text = |i: usize| tokens.get(i).map_or("", |t| &sql[t.start..t.end]);
    let word = |i: usize, name: &str| {
        tokens.get(i).is_some_and(|t| t.kind == Kind::Word) && text(i).eq_ignore_ascii_case(name)
    };
    for i in 0..tokens.len() {
        iso_type(sql, tokens, i, edits);
        match dialect {
            SqlDialect::Postgres => {
                if word(i, "ON") && word(i + 1, "CONFLICT") {
                    edits.extend(conflict_predicate(sql, tokens, i + 2));
                } else if word(i, "FOR") {
                    row_lock(sql, tokens, i, edits);
                } else if word(i, "OVERRIDING")
                    && (word(i + 1, "SYSTEM") || word(i + 1, "USER"))
                    && word(i + 2, "VALUE")
                {
                    edits.push((tokens[i].start, tokens[i + 2].end, String::new()));
                } else if POSTGRES_TYPED_STRINGS.iter().any(|ty| word(i, ty))
                    && text(i + 1).starts_with('\'')
                    && spaced(sql, tokens, i)
                {
                    let value = text(i + 1);
                    edits.push((
                        tokens[i].start,
                        tokens[i + 1].end,
                        format!("CAST({value} AS {})", text(i)),
                    ));
                }
            }
            SqlDialect::Sqlite => {
                if word(i, "INDEXED") && word(i + 1, "BY") && is_name(text(i + 2)) {
                    edits.push((tokens[i].start, tokens[i + 2].end, String::new()));
                } else if word(i, "NOT") && word(i + 1, "INDEXED") {
                    edits.push((tokens[i].start, tokens[i + 1].end, String::new()));
                }
            }
            _ => {}
        }
        // sqlparser reads `TABLE name` only after a set operator or a `WITH`.
        let set_operator = |j: usize| {
            ["UNION", "INTERSECT", "EXCEPT"]
                .iter()
                .any(|op| word(j, op))
        };
        if matches!(dialect, SqlDialect::Postgres | SqlDialect::MySql)
            && word(i, "TABLE")
            && is_name(text(i + 1))
            && (i == 0
                || matches!(tokens[i - 1].kind, Kind::Open | Kind::Close)
                || set_operator(i - 1)
                || ((word(i - 1, "ALL") || word(i - 1, "DISTINCT"))
                    && set_operator(i.wrapping_sub(2))))
        {
            edits.push((tokens[i].start, tokens[i].end, "SELECT * FROM".to_string()));
        }
    }
}

/// ISO types at token `i`: `BINARY LARGE OBJECT` as `BLOB`, `T MULTISET` as `T[]`.
fn iso_type(sql: &str, tokens: &[Token], i: usize, edits: &mut Vec<Edit>) {
    let word = |j: usize, name: &str| {
        tokens
            .get(j)
            .is_some_and(|t| t.kind == Kind::Word && sql[t.start..t.end].eq_ignore_ascii_case(name))
    };
    if word(i, "BINARY") && word(i + 1, "LARGE") && word(i + 2, "OBJECT") {
        edits.push((tokens[i].start, tokens[i + 2].end, "BLOB".to_string()));
    } else if word(i, "MULTISET") && i > 0 && spaced(sql, tokens, i - 1) {
        edits.push((tokens[i - 1].end, tokens[i].end, "[]".to_string()));
    }
}

/// Whether only whitespace, and some, separates token `i` from the next one.
fn spaced(sql: &str, tokens: &[Token], i: usize) -> bool {
    let Some(next) = tokens.get(i + 1) else {
        return false;
    };
    let gap = &sql[tokens[i].end..next.start];
    !gap.is_empty() && gap.bytes().all(|b| b.is_ascii_whitespace())
}

/// Whether a token is a plain or double-quoted name, or one in backticks.
fn is_name(text: &str) -> bool {
    text.starts_with(['"', '`'])
        || text
            .bytes()
            .next()
            .is_some_and(|b| b == b'_' || b.is_ascii_alphanumeric())
}

/// The edit dropping the `WHERE <predicate>` of `ON CONFLICT (cols) WHERE <predicate> DO`,
/// whose parenthesis is token `open`.
fn conflict_predicate(sql: &str, tokens: &[Token], open: usize) -> Option<Edit> {
    if tokens.get(open)?.kind != Kind::Open {
        return None;
    }
    let close = closing(tokens, open)?;
    let clause = tokens.get(close + 1)?;
    if !sql[clause.start..clause.end].eq_ignore_ascii_case("WHERE") {
        return None;
    }
    let mut depth = 0;
    for token in &tokens[close + 2..] {
        match token.kind {
            Kind::Open => depth += 1,
            Kind::Close if depth == 0 => return None,
            Kind::Close => depth -= 1,
            Kind::Word if depth == 0 && sql[token.start..token.end].eq_ignore_ascii_case("DO") => {
                return Some((tokens[close].end, token.start, " ".to_string()));
            }
            _ => {}
        }
    }
    None
}

/// PostgreSQL's row-lock clause at the `FOR` of token `i`, as sqlparser reads it: `NO KEY
/// UPDATE` as `UPDATE`, `KEY SHARE` as `SHARE`, and `OF` with only its first table.
fn row_lock(sql: &str, tokens: &[Token], i: usize, edits: &mut Vec<Edit>) {
    let text = |j: usize| tokens.get(j).map_or("", |t| &sql[t.start..t.end]);
    let word = |j: usize, name: &str| {
        tokens.get(j).is_some_and(|t| t.kind == Kind::Word) && text(j).eq_ignore_ascii_case(name)
    };
    let mode = if word(i + 1, "NO") && word(i + 2, "KEY") && word(i + 3, "UPDATE") {
        edits.push((tokens[i + 1].start, tokens[i + 3].start, String::new()));
        i + 3
    } else if word(i + 1, "KEY") && word(i + 2, "SHARE") {
        edits.push((tokens[i + 1].start, tokens[i + 2].start, String::new()));
        i + 2
    } else if word(i + 1, "UPDATE") || word(i + 1, "SHARE") {
        i + 1
    } else {
        return;
    };
    if !word(mode + 1, "OF") {
        return;
    }
    let name_end = |mut j: usize| {
        if !is_name(text(j)) {
            return None;
        }
        while text(j + 1) == "." && is_name(text(j + 2)) {
            j += 2;
        }
        Some(j)
    };
    let Some(first) = name_end(mode + 2) else {
        return;
    };
    let mut last = first;
    while tokens.get(last + 1).is_some_and(|t| t.kind == Kind::Comma) {
        match name_end(last + 2) {
            Some(end) => last = end,
            None => break,
        }
    }
    if last > first {
        edits.push((tokens[first].end, tokens[last].end, String::new()));
    }
}

/// psycopg placeholders as `NULL` and `%%` as `%`; in a string literal, only `%%`.
fn placeholders(sql: &str, tokens: &[Token], edits: &mut Vec<Edit>) {
    let bytes = sql.as_bytes();
    let mut quoted = tokens
        .iter()
        .filter(|t| t.kind == Kind::Other && t.end - t.start > 1)
        .peekable();
    let mut i = 0;
    while let Some(at) = sql[i..].find('%').map(|n| i + n) {
        while quoted.next_if(|t| t.end <= at).is_some() {}
        let in_literal = quoted.peek().is_some_and(|t| t.start < at);
        let rest = &bytes[at + 1..];
        let end = if rest.first() == Some(&b'%') {
            edits.push((at, at + 2, "%".to_string()));
            at + 2
        } else if in_literal {
            at + 1
        } else {
            let name = match rest.first() {
                Some(b'(') => sql[at + 2..].find(')').map_or(0, |n| n + 2),
                _ => 0,
            };
            match rest.get(name) {
                Some(b's' | b'b' | b't') if name != 2 => {
                    edits.push((at, at + name + 2, "NULL".to_string()));
                    at + name + 2
                }
                _ => at + 1,
            }
        };
        i = end;
    }
}
