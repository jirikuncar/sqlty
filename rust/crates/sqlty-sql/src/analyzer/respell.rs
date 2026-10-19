//! PostgreSQL and MySQL syntax sqlparser does not read, respelled as the closest syntax it
//! does and typed the same: `BETWEEN SYMMETRIC`, `AT LOCAL`, `COLLATION FOR`, `trim(both
//! from x, chars)`, `substring(x similar p escape e)`, the SQL/JSON clauses after a path,
//! `json_array(SELECT ...)`, and the comma-separated tables of MySQL's `UPDATE`. It runs
//! only on a query that does not parse as written.

use super::preprocess::quoted_end;
use super::star_expand::is_word;

/// `sql` with the syntax sqlparser rejects respelled; `None` when it has none.
pub(super) fn respell(sql: &str) -> Option<String> {
    let tokens = tokens(sql);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let word = |i: usize, name: &str| {
        tokens
            .get(i)
            .is_some_and(|t| t.kind == Kind::Word && sql[t.start..t.end].eq_ignore_ascii_case(name))
    };
    let is = |i: usize, kind: Kind| tokens.get(i).is_some_and(|t| t.kind == kind);
    for i in 0..tokens.len() {
        let call = |name: &str| word(i, name) && is(i + 1, Kind::Open);
        if word(i, "BETWEEN") && (word(i + 1, "SYMMETRIC") || word(i + 1, "ASYMMETRIC")) {
            edits.push((tokens[i + 1].start, tokens[i + 1].end, String::new()));
        } else if word(i, "AT") && word(i + 1, "LOCAL") {
            let local = &tokens[i + 1];
            edits.push((
                local.start,
                local.end,
                "TIME ZONE current_setting('TimeZone')".to_string(),
            ));
        } else if word(i, "COLLATION") && word(i + 1, "FOR") && is(i + 2, Kind::Open) {
            edits.push((
                tokens[i].start,
                tokens[i + 1].end,
                "pg_collation_for".to_string(),
            ));
        } else if call("TRIM") && word(i + 3, "FROM") {
            let trim = [
                ("BOTH", "btrim("),
                ("LEADING", "ltrim("),
                ("TRAILING", "rtrim("),
            ]
            .into_iter()
            .find(|(side, _)| word(i + 2, side));
            let chars = arguments(&tokens, i + 1)
                .iter()
                .any(|&j| tokens[j].kind == Kind::Comma);
            if let (Some((_, trim)), true) = (trim, chars) {
                edits.push((tokens[i].start, tokens[i + 3].end, trim.to_string()));
            }
        } else if call("SUBSTRING") {
            let inside = arguments(&tokens, i + 1);
            let similar = inside
                .iter()
                .find(|&&j| word(j, "SIMILAR") && !word(j + 1, "TO"));
            let escape = inside.iter().find(|&&j| word(j, "ESCAPE"));
            if let Some(&similar) = similar {
                edits.push((
                    tokens[similar].start,
                    tokens[similar].end,
                    "FROM".to_string(),
                ));
                if let Some(&escape) = escape {
                    edits.push((tokens[escape].start, tokens[escape].end, "FOR".to_string()));
                }
            }
        } else if call("JSON_EXISTS") || call("JSON_QUERY") || call("JSON_VALUE") {
            edits.extend(json_clauses(sql, &tokens, i + 1, word(i, "JSON_EXISTS")));
        } else if word(i, "UPDATE") && !word(i.wrapping_sub(1), "KEY") {
            edits.extend(update_tables(sql, &tokens, i));
        } else if call("JSON_ARRAY") && word(i + 2, "SELECT") {
            if let Some(close) = closing(&tokens, i + 1) {
                edits.push((tokens[i + 2].start, tokens[i + 2].start, "(".to_string()));
                edits.push((tokens[close].start, tokens[close].start, ")".to_string()));
            }
        }
    }
    if edits.is_empty() {
        return None;
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
    Some(out)
}

/// The clauses after the path of `JSON_EXISTS`, `JSON_QUERY` or `JSON_VALUE`, whose
/// parenthesis is token `open`, dropped but for `RETURNING type`. `UNKNOWN ON ERROR` makes
/// `JSON_EXISTS` nullable, so that call is left alone.
fn json_clauses(
    sql: &str,
    tokens: &[Token],
    open: usize,
    exists: bool,
) -> Option<(usize, usize, String)> {
    const CLAUSES: [&str; 12] = [
        "PASSING", "WITH", "WITHOUT", "KEEP", "OMIT", "ERROR", "NULL", "EMPTY", "DEFAULT", "TRUE",
        "FALSE", "UNKNOWN",
    ];
    let text = |j: usize| &sql[tokens[j].start..tokens[j].end];
    let inside = arguments(tokens, open);
    let comma = inside.iter().position(|&j| tokens[j].kind == Kind::Comma)?;
    let path = *inside.get(comma + 1)?;
    let close = closing(tokens, open)?;
    let rest = &inside[comma + 2..];
    let clause = |j: &usize| CLAUSES.iter().any(|c| text(*j).eq_ignore_ascii_case(c));
    if rest.is_empty()
        || tokens[path].kind != Kind::Other
        || (exists
            && rest
                .iter()
                .any(|&j| text(j).eq_ignore_ascii_case("UNKNOWN")))
    {
        return None;
    }
    let returning = rest
        .iter()
        .position(|&j| text(j).eq_ignore_ascii_case("RETURNING"))
        .map(|at| {
            let end = rest[at + 1..]
                .iter()
                .find(|j| clause(j))
                .map_or(tokens[close].start, |&j| tokens[j].start);
            format!(" {}", sql[tokens[rest[at]].start..end].trim_end())
        });
    Some((
        tokens[path].end,
        tokens[close].start,
        returning.unwrap_or_default(),
    ))
}

/// The tokens directly inside the parenthesis that opens at token `open`.
/// The commas between the tables of the `UPDATE` at token `update`, as cross joins.
fn update_tables(sql: &str, tokens: &[Token], update: usize) -> Vec<(usize, usize, String)> {
    let mut depth = 0;
    let mut commas = Vec::new();
    for token in &tokens[update + 1..] {
        match token.kind {
            Kind::Open => depth += 1,
            Kind::Close if depth == 0 => break,
            Kind::Close => depth -= 1,
            Kind::Comma if depth == 0 => {
                commas.push((token.start, token.end, " CROSS JOIN".to_string()));
            }
            Kind::Word if depth == 0 && sql[token.start..token.end].eq_ignore_ascii_case("SET") => {
                return commas;
            }
            Kind::Other if &sql[token.start..token.end] == ";" => break,
            _ => {}
        }
    }
    Vec::new()
}

fn arguments(tokens: &[Token], open: usize) -> Vec<usize> {
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
fn closing(tokens: &[Token], open: usize) -> Option<usize> {
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
enum Kind {
    Word,
    Open,
    Close,
    Comma,
    Other,
}

struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

/// The words, parentheses, commas and other tokens of `sql`, without whitespace and
/// comments; a quoted string or name is one `Other` token.
fn tokens(sql: &str) -> Vec<Token> {
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
            b'\'' | b'"' | b'`' => {
                i = quoted_end(bytes, i);
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
