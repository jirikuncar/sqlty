//! Schema DDL splitting and the constraint-free retry of a `CREATE TABLE`.
//!
//! Statements are split on `;` outside quotes, comments and dollar-quoted bodies.
//! A table sqlparser cannot read is retried without its constraints and `CHECK` /
//! `REFERENCES` clauses, which never change column types.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Space,
    Comment,
    Quoted,
    Word,
    Punct,
}

type Piece<'a> = (Kind, &'a str);

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// The end of the quoted run starting at `start`, which closes with `quote`
/// (doubled for an escape, or backslash-escaped when `backslash`).
fn quoted_end(bytes: &[u8], start: usize, quote: u8, backslash: bool) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if backslash => i += 2,
            b if b == quote && bytes.get(i + 1) == Some(&quote) => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// The end of a `$tag$ ... $tag$` body starting at `start`, if one starts there.
fn dollar_end(sql: &str, start: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    if start > 0 && (is_word_byte(bytes[start - 1]) || bytes[start - 1] == b'$') {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphabetic()
            || bytes[i] == b'_'
            || (i > start + 1 && bytes[i].is_ascii_digit()))
    {
        i += 1;
    }
    if bytes.get(i) != Some(&b'$') {
        return None;
    }
    let tag = &sql[start..=i];
    Some(
        sql[i + 1..]
            .find(tag)
            .map_or(sql.len(), |at| i + 1 + at + tag.len()),
    )
}

fn line_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |n| from + n)
}

fn block_comment_end(bytes: &[u8], from: usize) -> usize {
    let (mut depth, mut i) = (1, from + 2);
    while i < bytes.len() && depth > 0 {
        match bytes.get(i..i + 2) {
            Some(b"/*") => (depth, i) = (depth + 1, i + 2),
            Some(b"*/") => (depth, i) = (depth - 1, i + 2),
            _ => i += 1,
        }
    }
    i.min(bytes.len())
}

fn scan(sql: &str, mysql: bool) -> Vec<Piece<'_>> {
    let bytes = sql.as_bytes();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut i = 0;
    // Inside a MySQL executable comment (`/*!50001 CREATE VIEW ... */`), which MySQL runs.
    let mut executable = false;
    while i < bytes.len() {
        let b = bytes[i];
        let (kind, end) = match b {
            b'/' if mysql && bytes[i..].starts_with(b"/*!") => {
                executable = true;
                let digits = bytes[i + 3..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                (Kind::Comment, i + 3 + digits)
            }
            b'*' if executable && bytes.get(i + 1) == Some(&b'/') => {
                executable = false;
                (Kind::Comment, i + 2)
            }
            _ if b.is_ascii_whitespace() => {
                let len = bytes[i..]
                    .iter()
                    .take_while(|b| b.is_ascii_whitespace())
                    .count();
                (Kind::Space, i + len)
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => (Kind::Comment, line_end(bytes, i)),
            b'#' if mysql => (Kind::Comment, line_end(bytes, i)),
            b'/' if bytes.get(i + 1) == Some(&b'*') => (Kind::Comment, block_comment_end(bytes, i)),
            b'\'' => {
                let escaped = mysql
                    || matches!(pieces.last(), Some((Kind::Word, w)) if w.eq_ignore_ascii_case("e"));
                (Kind::Quoted, quoted_end(bytes, i, b'\'', escaped))
            }
            b'"' => (Kind::Quoted, quoted_end(bytes, i, b'"', false)),
            b'`' => (Kind::Quoted, quoted_end(bytes, i, b'`', false)),
            b'$' => match dollar_end(sql, i) {
                Some(end) => (Kind::Quoted, end),
                None => (Kind::Punct, i + 1),
            },
            _ if is_word_byte(b) => {
                let len = bytes[i..]
                    .iter()
                    .take_while(|&&b| is_word_byte(b) || b == b'$')
                    .count();
                (Kind::Word, i + len)
            }
            _ => (Kind::Punct, i + 1),
        };
        let end = end.min(bytes.len());
        pieces.push((kind, &sql[i..end]));
        i = end;
    }
    pieces
}

fn render(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|&(kind, text)| if kind == Kind::Comment { " " } else { text })
        .collect()
}

fn is_punct(piece: &Piece, ch: &str) -> bool {
    piece.0 == Kind::Punct && piece.1 == ch
}

fn is_word(piece: &Piece, word: &str) -> bool {
    piece.0 == Kind::Word && piece.1.eq_ignore_ascii_case(word)
}

fn significant(piece: &Piece) -> bool {
    !matches!(piece.0, Kind::Space | Kind::Comment)
}

/// One statement of the schema: the pieces between two `;`.
pub(crate) struct Statement<'a>(Vec<Piece<'a>>);

/// A `BEFORE ... FOR EACH ROW` trigger, which may change each row it fires for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RowTrigger {
    pub name: Vec<String>,
    pub table: Vec<String>,
    pub events: RowEvents,
}

/// The statements a row trigger fires for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowEvents {
    pub insert: bool,
    pub update: bool,
}

impl Statement<'_> {
    pub(crate) fn sql(&self) -> String {
        render(&self.0).trim().to_string()
    }

    /// Whether this is a `CREATE ... TABLE` with a column list.
    pub(crate) fn is_create_table(&self) -> bool {
        let mut sig = self.0.iter().filter(|p| significant(p));
        sig.next().is_some_and(|p| is_word(p, "create"))
            && sig
                .take_while(|p| p.0 == Kind::Word)
                .any(|p| is_word(p, "table"))
            && self.0.iter().any(|p| is_punct(p, "("))
    }

    /// Whether this is `CREATE [OR REPLACE] [TEMP | MATERIALIZED | ...] <object>`, e.g. `view`.
    pub(crate) fn creates(&self, object: &str) -> bool {
        const MODIFIERS: [&str; 7] = [
            "or",
            "replace",
            "temp",
            "temporary",
            "materialized",
            "recursive",
            "secure",
        ];
        let words: Vec<&Piece> = self.0.iter().filter(|p| significant(p)).collect();
        if !words.first().is_some_and(|p| is_word(p, "create")) {
            return false;
        }
        let mut at = 1;
        while let Some(word) = words.get(at) {
            let next = |n: usize, w: &str| words.get(at + n).is_some_and(|p| is_word(p, w));
            let eq = words.get(at + 1).is_some_and(|p| is_punct(p, "="));
            at += if MODIFIERS.iter().any(|m| is_word(word, m)) {
                1
            // MySQL's `ALGORITHM = UNDEFINED`, `SQL SECURITY DEFINER`.
            } else if (is_word(word, "algorithm") && eq)
                || (is_word(word, "sql") && next(1, "security"))
            {
                3
            // MySQL's `DEFINER = user@host` or `DEFINER = CURRENT_USER`.
            } else if is_word(word, "definer") && eq {
                if words.get(at + 3).is_some_and(|p| is_punct(p, "@")) {
                    5
                } else {
                    3
                }
            } else {
                return is_word(word, object);
            };
        }
        false
    }

    /// A `CREATE VIEW` without the `WITH [CASCADED | LOCAL] CHECK OPTION` that ends it,
    /// which sqlparser does not read; `None` when it has none.
    pub(crate) fn without_check_option(&self) -> Option<String> {
        let sig: Vec<usize> = (0..self.0.len())
            .filter(|&at| significant(&self.0[at]))
            .collect();
        let words = |from: usize, expected: &[&str]| {
            sig.len() >= from + expected.len()
                && expected
                    .iter()
                    .zip(&sig[sig.len() - from - expected.len()..])
                    .all(|(w, &at)| is_word(&self.0[at], w))
        };
        let start = [
            &["with", "check", "option"][..],
            &["with", "cascaded", "check", "option"],
            &["with", "local", "check", "option"],
        ]
        .iter()
        .find(|clause| words(0, clause))
        .map(|clause| sig[sig.len() - clause.len()])?;
        Some(render(&self.0[..start]).trim_end().to_string())
    }

    /// The name and base type of a `CREATE DOMAIN name [AS] type ...` as written, without
    /// the `COLLATE`, `DEFAULT` and constraint clauses that follow the type.
    pub(crate) fn domain(&self) -> Option<(String, String)> {
        const CLAUSES: [&str; 6] = ["collate", "default", "constraint", "check", "not", "null"];
        let pieces: Vec<&Piece> = self.0.iter().filter(|p| significant(p)).collect();
        let name_at = pieces.iter().position(|p| is_word(p, "domain"))? + 1;
        let name_len = pieces[name_at..]
            .iter()
            .enumerate()
            .take_while(|(at, p)| {
                if at % 2 == 0 {
                    matches!(p.0, Kind::Word | Kind::Quoted)
                } else {
                    is_punct(p, ".")
                }
            })
            .count();
        let type_at = name_at
            + name_len
            + usize::from(
                pieces
                    .get(name_at + name_len)
                    .is_some_and(|p| is_word(p, "as")),
            );
        let type_len = pieces[type_at..]
            .iter()
            .position(|p| p.0 == Kind::Word && CLAUSES.iter().any(|c| is_word(p, c)))
            .unwrap_or(pieces.len() - type_at);
        let text = |range: &[&Piece]| range.iter().map(|p| p.1).collect::<Vec<_>>().join(" ");
        (name_len > 0 && type_len > 0).then(|| {
            (
                pieces[name_at..name_at + name_len]
                    .iter()
                    .map(|p| p.1)
                    .collect(),
                text(&pieces[type_at..type_at + type_len]),
            )
        })
    }

    /// The conditions of the `CHECK (...)` clauses of a `CREATE DOMAIN`, without the
    /// `CHECK` and the parentheses around them.
    pub(crate) fn domain_checks(&self) -> Vec<String> {
        let spots: Vec<usize> = (0..self.0.len())
            .filter(|&at| significant(&self.0[at]))
            .collect();
        let pieces: Vec<&Piece> = spots.iter().map(|&at| &self.0[at]).collect();
        let mut checks = Vec::new();
        let mut at = 0;
        while at + 1 < pieces.len() {
            if !(is_word(pieces[at], "check") && is_punct(pieces[at + 1], "(")) {
                at += 1;
                continue;
            }
            let mut depth = 0usize;
            let mut end = None;
            for (offset, piece) in pieces[at + 1..].iter().enumerate() {
                if is_punct(piece, "(") {
                    depth += 1;
                } else if is_punct(piece, ")") {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(at + 1 + offset);
                        break;
                    }
                }
            }
            let Some(end) = end else { break };
            checks.push(render(&self.0[spots[at + 1] + 1..spots[end]]));
            at = end + 1;
        }
        checks
    }

    /// The lowercase words that open the statement, up to `n` (`create`, `or`, `replace`, ...).
    #[cfg(test)]
    pub(crate) fn lead_words(&self, n: usize) -> Vec<String> {
        self.0
            .iter()
            .filter(|p| significant(p))
            .map_while(|p| (p.0 == Kind::Word).then(|| p.1.to_lowercase()))
            .take(n)
            .collect()
    }

    /// Whether the lowercase `word` appears outside quotes and comments.
    pub(crate) fn has_word(&self, word: &str) -> bool {
        self.0.iter().any(|p| is_word(p, word))
    }

    /// Whether this is a `CREATE ... TABLE` with or without a column list, such as
    /// `CREATE TABLE p PARTITION OF parent DEFAULT`.
    pub(crate) fn is_create_table_any(&self) -> bool {
        let mut sig = self.0.iter().filter(|p| significant(p));
        sig.next().is_some_and(|p| is_word(p, "create"))
            && sig
                .take_while(|p| p.0 == Kind::Word)
                .any(|p| is_word(p, "table"))
    }

    /// The `(kind, name parts)` an `ALTER` or `DROP` statement targets, e.g.
    /// `("table", ["app", "users"])` for `ALTER TABLE IF EXISTS ONLY app.users ...`.
    /// Parts are unquoted; an unquoted part is lowercased, a quoted one keeps its case. `DROP` lists give the first name.
    pub(crate) fn target(&self, verb: &str) -> Option<(String, Vec<String>)> {
        self.target_and_action(verb)
            .map(|(kind, name, _)| (kind, name))
    }

    /// [`Statement::target`], and the (up to two) lowercase words after the name,
    /// e.g. `["owner", "to"]` for `ALTER TABLE t OWNER TO me`.
    pub(crate) fn target_and_action(
        &self,
        verb: &str,
    ) -> Option<(String, Vec<String>, Vec<String>)> {
        let pieces: Vec<&Piece> = self.0.iter().filter(|p| significant(p)).collect();
        if !pieces.first().is_some_and(|p| is_word(p, verb)) {
            return None;
        }
        let mut at = 1;
        let mut kind = pieces.get(at)?.1.to_lowercase();
        at += 1;
        if kind == "materialized" {
            kind = pieces.get(at)?.1.to_lowercase();
            at += 1;
        }
        for skip in [&["if", "exists"][..], &["only"][..]] {
            if skip
                .iter()
                .enumerate()
                .all(|(i, w)| pieces.get(at + i).is_some_and(|p| is_word(p, w)))
            {
                at += skip.len();
            }
        }
        let (name, used) = name_parts(&pieces[at..]);
        let action = pieces[at + used..]
            .iter()
            .map_while(|p| (p.0 == Kind::Word).then(|| p.1.to_lowercase()))
            .take(2)
            .collect();
        (!name.is_empty()).then_some((kind, name, action))
    }

    /// A `CREATE TRIGGER name BEFORE INSERT OR UPDATE ... ON table ... FOR EACH ROW`: a
    /// trigger that may set the columns of each row it inserts or updates.
    pub(crate) fn before_row_trigger(&self) -> Option<RowTrigger> {
        let pieces: Vec<&Piece> = self.0.iter().filter(|p| significant(p)).collect();
        let mut at = pieces.iter().position(|p| is_word(p, "trigger"))? + 1;
        // Not the first word: MySQL's `DEFINER = ...`, or a `DELIMITER //` line before it.
        if !pieces[..at].iter().any(|p| is_word(p, "create")) {
            return None;
        }
        if pieces
            .get(at..at + 3)
            .is_some_and(|w| is_word(w[0], "if") && is_word(w[1], "not") && is_word(w[2], "exists"))
        {
            at += 3;
        }
        let (name, used) = name_parts(&pieces[at..]);
        at += used;
        if !pieces.get(at).is_some_and(|p| is_word(p, "before")) {
            return None;
        }
        let on = at + pieces[at..].iter().position(|p| is_word(p, "on"))?;
        let for_each_row = pieces[on..]
            .windows(3)
            .any(|w| is_word(w[0], "for") && is_word(w[1], "each") && is_word(w[2], "row"));
        let fires_on = |event| pieces[at..on].iter().any(|p| is_word(p, event));
        let events = RowEvents {
            insert: fires_on("insert"),
            update: fires_on("update"),
        };
        let table = name_parts(&pieces[on + 1..]).0;
        (for_each_row && (events.insert || events.update) && !table.is_empty()).then_some(
            RowTrigger {
                name,
                table,
                events,
            },
        )
    }

    /// The name parts after the first `keyword` (`function` in `CREATE FUNCTION a.f(...)`).
    pub(crate) fn name_parts_after(&self, keyword: &str) -> Vec<String> {
        let pieces: Vec<&Piece> = self.0.iter().filter(|p| significant(p)).collect();
        let Some(at) = pieces.iter().position(|p| is_word(p, keyword)) else {
            return Vec::new();
        };
        let mut at = at + 1;
        if pieces
            .get(at..at + 3)
            .is_some_and(|w| is_word(w[0], "if") && is_word(w[1], "not") && is_word(w[2], "exists"))
        {
            at += 3;
        }
        name_parts(&pieces[at..]).0
    }

    /// The schemas a `SET search_path TO a, b` (or `set_config('search_path', 'a, b', ...)`)
    /// statement sets; `DEFAULT` gives `["public"]`.
    pub(crate) fn search_path(&self) -> Option<Vec<String>> {
        let pieces: Vec<&Piece> = self.0.iter().filter(|p| significant(p)).collect();
        let first = pieces.first()?;
        if is_word(first, "select") {
            let text = self.sql();
            let caps = SET_CONFIG.captures(&text)?;
            return Some(path_list(&caps[1]));
        }
        if !is_word(first, "set") {
            return None;
        }
        let mut at = 1;
        if pieces
            .get(at)
            .is_some_and(|p| is_word(p, "session") || is_word(p, "local"))
        {
            at += 1;
        }
        if !pieces.get(at).is_some_and(|p| is_word(p, "search_path")) {
            return None;
        }
        at += 1;
        if !pieces
            .get(at)
            .is_some_and(|p| is_word(p, "to") || is_punct(p, "="))
        {
            return None;
        }
        let items: Vec<String> = pieces[at + 1..]
            .iter()
            .filter(|p| !is_punct(p, ","))
            .map(|p| unquote(p.1))
            .collect();
        if items.len() == 1 && items[0] == "default" && pieces[at + 1].0 == Kind::Word {
            return Some(vec!["public".to_string()]);
        }
        Some(items)
    }

    /// The table name as written, e.g. `public.users`.
    pub(crate) fn table_name(&self) -> String {
        self.name_after("table", |p| is_punct(p, "("))
    }

    /// The view name as written, e.g. `public.recent`.
    pub(crate) fn view_name(&self) -> String {
        self.name_after("view", |p| {
            is_punct(p, "(") || is_word(p, "as") || is_word(p, "comment")
        })
    }

    fn name_after(&self, keyword: &str, ends: impl Fn(&Piece) -> bool) -> String {
        let start = self
            .0
            .iter()
            .position(|p| is_word(p, keyword))
            .map_or(0, |at| at + 1);
        let end = self
            .0
            .iter()
            .skip(start)
            .position(ends)
            .map_or(self.0.len(), |at| start + at);
        let name = render(&self.0[start..end]);
        let name = name.trim();
        let prefix = "if not exists";
        match name.get(..prefix.len()) {
            Some(head) if head.eq_ignore_ascii_case(prefix) => name[prefix.len()..].trim(),
            _ => name,
        }
        .to_string()
    }

    /// The statement without its `LIKE source [INCLUDING ...]` items, and the sources'
    /// name parts; `None` when it has none. sqlparser cannot read the `INCLUDING` options.
    pub(crate) fn without_like(&self) -> Option<(String, Vec<Vec<String>>)> {
        let pieces = &self.0;
        let open = pieces.iter().position(|p| is_punct(p, "("))?;
        let close = matching_paren(pieces, open)?;
        let mut kept = Vec::new();
        let mut sources = Vec::new();
        for item in split_top_level(&pieces[open + 1..close], ",") {
            let sig: Vec<&Piece> = item.iter().filter(|p| significant(p)).collect();
            if sig.first().is_some_and(|p| is_word(p, "like")) {
                sources.push(name_parts(&sig[1..]).0);
            } else {
                kept.push(render(item));
            }
        }
        (!sources.is_empty()).then(|| {
            (
                format!(
                    "{}{}{}",
                    render(&pieces[..=open]),
                    kept.join(","),
                    render(&pieces[close..])
                ),
                sources,
            )
        })
    }

    /// The statement without the `PARTITION BY ...` that follows its column list, which
    /// sqlparser does not read for MySQL; `None` when it has none.
    pub(crate) fn without_partitioning(&self) -> Option<String> {
        let pieces = &self.0;
        let open = pieces.iter().position(|p| is_punct(p, "("))?;
        let close = matching_paren(pieces, open)?;
        let sig: Vec<usize> = (close + 1..pieces.len())
            .filter(|&at| significant(&pieces[at]))
            .collect();
        let at = sig.windows(2).find_map(|pair| {
            (is_word(&pieces[pair[0]], "partition") && is_word(&pieces[pair[1]], "by"))
                .then_some(pair[0])
        })?;
        Some(render(&pieces[..at]).trim_end().to_string())
    }

    /// The statement without table constraints and column `CHECK` / `REFERENCES`
    /// clauses, or `None` when it has none to drop.
    pub(crate) fn without_constraints(&self) -> Option<String> {
        let pieces = &self.0;
        let open = pieces.iter().position(|p| is_punct(p, "("))?;
        let close = matching_paren(pieces, open)?;
        let mut items = Vec::new();
        let mut changed = false;
        for item in split_top_level(&pieces[open + 1..close], ",") {
            let Some(lead) = item.iter().find(|p| significant(p)) else {
                continue;
            };
            let table_level = lead.0 == Kind::Word
                && [
                    "constraint",
                    "check",
                    "foreign",
                    "unique",
                    "primary",
                    "exclude",
                ]
                .iter()
                .any(|kw| lead.1.eq_ignore_ascii_case(kw));
            if table_level {
                changed = true;
                continue;
            }
            let (kept, dropped) = without_column_clauses(item);
            changed |= dropped;
            items.push(kept);
        }
        changed.then(|| {
            format!(
                "{}{}{}",
                render(&pieces[..=open]),
                items.join(","),
                render(&pieces[close..])
            )
        })
    }
}

static SET_CONFIG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)\bset_config\s*\(\s*'search_path'\s*,\s*'([^']*)'").unwrap()
});

/// A comma-separated `search_path` value such as `app, "$user", public`.
fn path_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(unquote)
        .filter(|name| !name.is_empty())
        .collect()
}

/// An identifier or string literal without its quotes, lowercased unless quoted.
fn unquote(text: &str) -> String {
    let text = text.trim();
    for quote in ['"', '`', '\''] {
        if let Some(inner) = text
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.replace(&format!("{quote}{quote}"), &quote.to_string());
        }
    }
    text.to_lowercase()
}

/// The dotted name at the start of `pieces`: words or quoted names joined by `.`.
fn name_parts(pieces: &[&Piece]) -> (Vec<String>, usize) {
    let mut parts = Vec::new();
    let mut expect_name = true;
    let mut used = 0;
    for piece in pieces {
        if expect_name && matches!(piece.0, Kind::Word | Kind::Quoted) {
            parts.push(unquote(piece.1));
            expect_name = false;
        } else if !expect_name && is_punct(piece, ".") {
            expect_name = true;
        } else {
            break;
        }
        used += 1;
    }
    (parts, used)
}

/// The statements of `sql`, split on `;` outside quotes, comments and dollar-quoted
/// bodies; `mysql` also reads backslash escapes and `#` comments.
pub(crate) fn split_statements(sql: &str, mysql: bool) -> Vec<Statement<'_>> {
    let mut parts: Vec<Vec<Piece>> = vec![Vec::new()];
    for piece in scan(sql, mysql) {
        if is_punct(&piece, ";") {
            parts.push(Vec::new());
        } else {
            parts.last_mut().unwrap().push(piece);
        }
    }
    parts
        .into_iter()
        .filter(|part| part.iter().any(significant))
        .map(Statement)
        .collect()
}

/// Split on `sep` outside parentheses.
fn split_top_level<'a, 'b>(pieces: &'b [Piece<'a>], sep: &str) -> Vec<&'b [Piece<'a>]> {
    let (mut parts, mut depth, mut from) = (Vec::new(), 0usize, 0);
    for (at, piece) in pieces.iter().enumerate() {
        if is_punct(piece, "(") {
            depth += 1;
        } else if is_punct(piece, ")") {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && is_punct(piece, sep) {
            parts.push(&pieces[from..at]);
            from = at + 1;
        }
    }
    parts.push(&pieces[from..]);
    parts
}

/// The index of the `)` closing the `(` at `open`.
fn matching_paren(pieces: &[Piece], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, piece) in pieces.iter().enumerate().skip(open) {
        if is_punct(piece, "(") {
            depth += 1;
        } else if is_punct(piece, ")") {
            depth -= 1;
            if depth == 0 {
                return Some(at);
            }
        }
    }
    None
}

/// A column definition without its `CHECK (...)`, `REFERENCES ...` and the
/// `CONSTRAINT name` before them, and whether anything was dropped.
fn without_column_clauses(item: &[Piece]) -> (String, bool) {
    let sig: Vec<usize> = (0..item.len())
        .filter(|&at| significant(&item[at]))
        .collect();
    let cols = Cols { item, sig: &sig };
    let mut drop = vec![false; item.len()];
    let mut n = 0;
    while n < sig.len() {
        let at = if cols.word(n, "constraint") && cols.word(n + 2, "check")
            || cols.word(n, "constraint") && cols.word(n + 2, "references")
        {
            n + 2
        } else {
            n
        };
        let end = if cols.word(at, "check") {
            cols.group(at + 1).filter(|&end| end > at + 1)
        } else if cols.word(at, "references") {
            Some(cols.references_end(at))
        } else {
            None
        };
        match end {
            Some(end) => {
                for &piece in &sig[n..end] {
                    drop[piece] = true;
                }
                n = end;
            }
            None => n += 1,
        }
    }
    let kept = item
        .iter()
        .zip(&drop)
        .filter(|(_, &dropped)| !dropped)
        .map(|(&(kind, text), _)| if kind == Kind::Comment { " " } else { text })
        .collect();
    (kept, drop.contains(&true))
}

/// The significant tokens of one column definition.
struct Cols<'a, 'b> {
    item: &'b [Piece<'a>],
    sig: &'b [usize],
}

impl Cols<'_, '_> {
    fn word(&self, n: usize, w: &str) -> bool {
        self.sig
            .get(n)
            .is_some_and(|&at| is_word(&self.item[at], w))
    }

    fn punct(&self, n: usize, ch: &str) -> bool {
        self.sig
            .get(n)
            .is_some_and(|&at| is_punct(&self.item[at], ch))
    }

    /// The index after the parenthesised group at `n`, if one starts there.
    fn group(&self, n: usize) -> Option<usize> {
        let close = matching_paren(self.item, *self.sig.get(n).filter(|_| self.punct(n, "("))?)?;
        self.sig.iter().position(|&at| at == close).map(|p| p + 1)
    }

    /// The index after the `REFERENCES` clause starting at `n`.
    fn references_end(&self, mut n: usize) -> usize {
        n += 1;
        loop {
            n += 1;
            if !self.punct(n, ".") {
                break;
            }
            n += 1;
        }
        n = self.group(n).unwrap_or(n);
        loop {
            if self.word(n, "match") || self.word(n, "initially") {
                n += 2;
            } else if self.word(n, "on")
                && (self.word(n + 1, "delete") || self.word(n + 1, "update"))
            {
                n += 2 + usize::from(self.word(n + 2, "set") || self.word(n + 2, "no")) + 1;
                n = self.group(n).unwrap_or(n);
            } else if self.word(n, "deferrable") {
                n += 1;
            } else if self.word(n, "not") && self.word(n + 1, "deferrable") {
                n += 2;
            } else {
                return n.min(self.sig.len());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqls(sql: &str) -> Vec<String> {
        split_statements(sql, false)
            .iter()
            .map(Statement::sql)
            .collect()
    }

    fn stripped(sql: &str) -> Option<String> {
        let sql = split_statements(sql, false)[0].without_constraints()?;
        Some(sql.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    #[test]
    fn test_split_respects_quotes_and_bodies() {
        assert_eq!(
            sqls("SELECT ';' ; -- a;b\nCREATE FUNCTION f() AS $$ x; y; $$; /* ; */ SELECT $t$;$t$"),
            [
                "SELECT ';'",
                "CREATE FUNCTION f() AS $$ x; y; $$",
                "SELECT $t$;$t$"
            ]
        );
        assert_eq!(
            sqls("SELECT $1; SELECT 'it''s;'"),
            ["SELECT $1", "SELECT 'it''s;'"]
        );
        assert_eq!(
            sqls("SELECT E'a\\';b'; SELECT 1"),
            ["SELECT E'a\\';b'", "SELECT 1"]
        );
    }

    #[test]
    fn test_creates_and_domain() {
        let first = |sql| split_statements(sql, false).remove(0);
        assert!(first("CREATE OR REPLACE MATERIALIZED VIEW v AS SELECT 1").creates("view"));
        assert!(!first("CREATE TABLE view (a INT)").creates("view"));
        assert!(first("create domain a.b as numeric(5, 2) not null default 0").creates("domain"));
        assert_eq!(
            first("CREATE DOMAIN app.\"p\" AS numeric(5, 2) NOT NULL DEFAULT 0").domain(),
            Some(("app.\"p\"".to_string(), "numeric ( 5 , 2 )".to_string()))
        );
        assert_eq!(
            first("CREATE DOMAIN d text COLLATE \"C\" CHECK (VALUE <> '')").domain(),
            Some(("d".to_string(), "text".to_string()))
        );
        assert_eq!(first("CREATE DOMAIN d").domain(), None);
    }

    #[test]
    fn test_domain_checks() {
        let first = |sql| split_statements(sql, false).remove(0);
        assert_eq!(
            first(
                "CREATE DOMAIN d text CHECK (VALUE IN ('a', 'b)')) CONSTRAINT c CHECK ((VALUE)::text <> 'x')"
            )
            .domain_checks(),
            vec!["VALUE IN ('a', 'b)')", "(VALUE)::text <> 'x'"]
        );
        assert!(first("CREATE DOMAIN d text NOT NULL")
            .domain_checks()
            .is_empty());
    }

    #[test]
    fn test_without_constraints() {
        assert_eq!(
            stripped(
                "CREATE TABLE t (a INT CHECK (a > 0) NOT NULL, \
                 b INT REFERENCES u(id) ON DELETE SET NULL (b) NOT NULL, c TEXT, \
                 CONSTRAINT k CHECK (length(c) > 0), PRIMARY KEY (a))"
            )
            .as_deref(),
            Some("CREATE TABLE t (a INT NOT NULL, b INT NOT NULL, c TEXT)")
        );
        assert_eq!(stripped("CREATE TABLE t (a INT, b TEXT)"), None);
        assert_eq!(
            stripped(
                "CREATE TABLE t (a INT CONSTRAINT c REFERENCES public.u (id) MATCH FULL \
                 ON UPDATE CASCADE DEFERRABLE INITIALLY DEFERRED DEFAULT 1)"
            )
            .as_deref(),
            Some("CREATE TABLE t (a INT DEFAULT 1)")
        );
    }

    #[test]
    fn test_mysql_views() {
        let sql = "/*!50001 CREATE ALGORITHM=UNDEFINED */ /*!50013 DEFINER=`root`@`%` \
                   SQL SECURITY DEFINER */ /*!50001 VIEW `v` AS select 1 AS `a` */ \
                   /*!50002 WITH CASCADED CHECK OPTION */";
        let stmt = &split_statements(sql, true)[0];
        assert!(stmt.creates("view"));
        assert_eq!(stmt.view_name(), "`v`");
        let view = stmt.without_check_option().unwrap();
        assert_eq!(
            view.split_whitespace().collect::<Vec<_>>().join(" "),
            "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`%` SQL SECURITY DEFINER VIEW `v` AS select 1 AS `a`"
        );
        let stmt = &split_statements("CREATE DEFINER = CURRENT_USER VIEW v AS SELECT 1", true)[0];
        assert!(stmt.creates("view"));
        assert_eq!(stmt.without_check_option(), None);
        assert!(split_statements("/* CREATE VIEW v AS SELECT 1 */", true).is_empty());
    }

    #[test]
    fn test_without_partitioning() {
        let sql = "CREATE TABLE t (id INT, PRIMARY KEY (id)) ENGINE=InnoDB \
                   PARTITION BY RANGE (id) (PARTITION p0 VALUES LESS THAN (10))";
        assert_eq!(
            split_statements(sql, true)[0]
                .without_partitioning()
                .as_deref(),
            Some("CREATE TABLE t (id INT, PRIMARY KEY (id)) ENGINE=InnoDB")
        );
        let sql = "CREATE TABLE t (id INT)";
        assert_eq!(split_statements(sql, true)[0].without_partitioning(), None);
    }

    fn first(sql: &str) -> Statement<'_> {
        split_statements(sql, false).remove(0)
    }

    #[test]
    fn test_target_and_action() {
        let target = |sql: &str, verb: &str| first(sql).target_and_action(verb);
        assert_eq!(
            target(
                "ALTER TABLE IF EXISTS ONLY app.\"Users\" OWNER TO me",
                "alter"
            ),
            Some((
                "table".to_string(),
                vec!["app".to_string(), "Users".to_string()],
                vec!["owner".to_string(), "to".to_string()]
            ))
        );
        assert_eq!(
            target("DROP MATERIALIZED VIEW IF EXISTS v CASCADE", "drop").map(|t| (t.0, t.1)),
            Some(("view".to_string(), vec!["v".to_string()]))
        );
        assert_eq!(target("CREATE TABLE t (a int)", "alter"), None);
    }

    #[test]
    fn test_search_path() {
        let path = |sql: &str| first(sql).search_path();
        let list = |names: &[&str]| Some(names.iter().map(ToString::to_string).collect());
        assert_eq!(
            path("SET search_path TO app, \"$user\", Public"),
            list(&["app", "$user", "public"])
        );
        assert_eq!(path("SET LOCAL search_path = 'a'"), list(&["a"]));
        assert_eq!(path("SET search_path TO DEFAULT"), list(&["public"]));
        assert_eq!(
            path("SELECT pg_catalog.set_config('search_path', 'a, b', false)"),
            list(&["a", "b"])
        );
        assert_eq!(
            path("SELECT pg_catalog.set_config('search_path', '', false)"),
            Some(vec![])
        );
        assert_eq!(path("SET statement_timeout = 0"), None);
        assert_eq!(path("SELECT 1"), None);
    }

    #[test]
    fn test_without_like() {
        let stmt = first("CREATE TABLE t (LIKE app.a INCLUDING ALL, x int, LIKE b)");
        let (sql, likes) = stmt.without_like().unwrap();
        assert_eq!(
            sql.split_whitespace().collect::<String>(),
            "CREATETABLEt(xint)"
        );
        assert_eq!(
            likes,
            vec![
                vec!["app".to_string(), "a".to_string()],
                vec!["b".to_string()]
            ]
        );
        assert!(first("CREATE TABLE t (x int)").without_like().is_none());
    }

    #[test]
    fn test_create_table_any_and_lead_words() {
        assert!(first("CREATE TABLE p PARTITION OF q DEFAULT").is_create_table_any());
        assert!(!first("CREATE TABLE p PARTITION OF q DEFAULT").is_create_table());
        assert!(!first("CREATE VIEW v AS SELECT 1").is_create_table_any());
        assert_eq!(
            first("create or replace function f()").lead_words(3),
            ["create", "or", "replace"]
        );
    }
}
