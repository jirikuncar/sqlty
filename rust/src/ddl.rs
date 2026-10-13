//! Schema DDL splitting and the constraint-free retry of a `CREATE TABLE`.
//!
//! Statements are split on `;` outside quotes, comments and dollar-quoted bodies.
//! A table sqlparser cannot read is retried without its constraints and `CHECK` /
//! `REFERENCES` clauses, which never change column types.

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
    while i < bytes.len() {
        let b = bytes[i];
        let (kind, end) = match b {
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
pub struct Statement<'a>(Vec<Piece<'a>>);

impl Statement<'_> {
    pub fn sql(&self) -> String {
        render(&self.0).trim().to_string()
    }

    /// Whether this is a `CREATE ... TABLE` with a column list.
    pub fn is_create_table(&self) -> bool {
        let mut sig = self.0.iter().filter(|p| significant(p));
        sig.next().is_some_and(|p| is_word(p, "create"))
            && sig
                .take_while(|p| p.0 == Kind::Word)
                .any(|p| is_word(p, "table"))
            && self.0.iter().any(|p| is_punct(p, "("))
    }

    /// Whether this is `CREATE [OR REPLACE] [TEMP | MATERIALIZED | ...] <object>`, e.g. `view`.
    pub fn creates(&self, object: &str) -> bool {
        const MODIFIERS: [&str; 7] = [
            "or",
            "replace",
            "temp",
            "temporary",
            "materialized",
            "recursive",
            "secure",
        ];
        let mut words = self.0.iter().filter(|p| significant(p));
        words.next().is_some_and(|p| is_word(p, "create"))
            && words
                .find(|p| !MODIFIERS.iter().any(|m| is_word(p, m)))
                .is_some_and(|p| is_word(p, object))
    }

    /// The name and base type of a `CREATE DOMAIN name [AS] type ...` as written, without
    /// the `COLLATE`, `DEFAULT` and constraint clauses that follow the type.
    pub fn domain(&self) -> Option<(String, String)> {
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

    /// The table name as written, e.g. `public.users`.
    pub fn table_name(&self) -> String {
        let start = self
            .0
            .iter()
            .position(|p| is_word(p, "table"))
            .map_or(0, |at| at + 1);
        let end = self
            .0
            .iter()
            .position(|p| is_punct(p, "("))
            .unwrap_or(self.0.len())
            .max(start);
        let name = render(&self.0[start..end]);
        let name = name.trim();
        let prefix = "if not exists";
        match name.get(..prefix.len()) {
            Some(head) if head.eq_ignore_ascii_case(prefix) => name[prefix.len()..].trim(),
            _ => name,
        }
        .to_string()
    }

    /// The statement without table constraints and column `CHECK` / `REFERENCES`
    /// clauses, or `None` when it has none to drop.
    pub fn without_constraints(&self) -> Option<String> {
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

/// The statements of `sql`, split on `;` outside quotes, comments and dollar-quoted
/// bodies; `mysql` also reads backslash escapes and `#` comments.
pub fn split_statements(sql: &str, mysql: bool) -> Vec<Statement<'_>> {
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
}
