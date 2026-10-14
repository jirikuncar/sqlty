//! Just enough of a Python tokenizer to find string literals, comments and
//! `{wrapper}[T](...)` calls, so edits never cut a call or a string in two.
//!
//! f- and t-string replacement fields are lexed (PEP 701 nesting included),
//! and bracket depth is reset at a line that can only start a statement, so a
//! lexing gap can't shift the depth for the rest of the file.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A string literal, with its prefix.
    Str,
    Comment,
    /// A dotted name, keyword or number.
    Name,
    /// The end of a logical line.
    Newline,
    Op,
    /// An unterminated string, up to the end of its line.
    Error,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
    /// Bracket depth; a bracket and its match share it.
    pub depth: usize,
}

impl Token {
    /// The prefix of a string literal (`""`, `u`, `rb`, `f`, ...), lowercased.
    pub(crate) fn prefix(&self, source: &str) -> String {
        let text = &source[self.span.clone()];
        text[..text.find(['"', '\'']).unwrap_or(0)].to_ascii_lowercase()
    }

    /// A string literal with no prefix but `u`, whose source text is its content.
    pub(crate) fn is_plain(&self, source: &str) -> bool {
        self.kind == Kind::Str && matches!(self.prefix(source).as_str(), "" | "u")
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' || byte >= 0x80
}

fn is_string_prefix(prefix: &str) -> bool {
    matches!(
        prefix.to_ascii_lowercase().as_str(),
        "u" | "r" | "b" | "f" | "t" | "br" | "rb" | "fr" | "rf" | "tr" | "rt"
    )
}

fn has_fields(prefix: &str) -> bool {
    prefix.contains(['f', 'F', 't', 'T'])
}

/// End of the string literal whose opening quote is at `at`, if it is closed.
fn string_end(bytes: &[u8], at: usize, fields: bool) -> Option<usize> {
    let quote = bytes[at];
    let len = if bytes[at..].starts_with(&[quote; 3]) {
        3
    } else {
        1
    };
    let mut i = at + len;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if fields && matches!(bytes.get(i + 1), Some(b'{' | b'}')) => i += 1,
            b'\\' if bytes[i + 1..].starts_with(b"\r\n") => i += 3,
            b'\\' => i += 2,
            b'\n' if len == 1 => return None,
            b'{' if fields && bytes.get(i + 1) == Some(&b'{') => i += 2,
            b'{' if fields => i = field_end(bytes, i + 1, len == 3)?,
            byte if byte == quote && bytes[i..].starts_with(&bytes[at..at + len]) => {
                return Some(i + len)
            }
            _ => i += 1,
        }
    }
    None
}

/// End of an f-string replacement field whose expression starts at `i`; `triple`
/// when the f-string is triple-quoted.
fn field_end(bytes: &[u8], mut i: usize, triple: bool) -> Option<usize> {
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => i = string_end(bytes, i, false)?,
            byte if is_name_byte(byte) => {
                let start = i;
                while i < bytes.len() && is_name_byte(bytes[i]) {
                    i += 1;
                }
                let prefix = std::str::from_utf8(&bytes[start..i]).unwrap_or_default();
                if matches!(bytes.get(i), Some(b'"' | b'\'')) && is_string_prefix(prefix) {
                    i = string_end(bytes, i, has_fields(prefix))?;
                }
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' if depth == 0 => return Some(i + 1),
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b':' if depth == 0 => return spec_end(bytes, i + 1, triple),
            b'#' => {
                i = bytes[i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(bytes.len(), |n| i + n);
            }
            _ => i += 1,
        }
    }
    None
}

/// End of a format spec starting at `i`, with nested replacement fields.
fn spec_end(bytes: &[u8], mut i: usize, triple: bool) -> Option<usize> {
    while i < bytes.len() {
        match bytes[i] {
            b'{' => i = field_end(bytes, i + 1, triple)?,
            b'}' => return Some(i + 1),
            b'\n' if !triple => return None,
            _ => i += 1,
        }
    }
    None
}

/// Whether the line at `at` can only start a statement, so it can't continue
/// a bracketed expression.
fn starts_statement(source: &str, at: usize) -> bool {
    let line = &source[at..source[at..].find('\n').map_or(source.len(), |n| at + n)];
    let word = line
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or_default();
    matches!(word, "def" | "class" | "import") || (word == "from" && line.contains(" import "))
}

/// The token starting at `i`, a byte that is no whitespace.
fn lex(source: &str, i: usize) -> (Kind, usize) {
    let bytes = source.as_bytes();
    let line_end = || source[i..].find('\n').map_or(bytes.len(), |n| i + n);
    let string = |at: usize, fields: bool| match string_end(bytes, at, fields) {
        Some(end) => (Kind::Str, end),
        None => (Kind::Error, line_end().max(at + 1)),
    };
    match bytes[i] {
        b'\n' => (Kind::Newline, i + 1),
        b'#' => (Kind::Comment, line_end()),
        b'"' | b'\'' => string(i, false),
        byte if is_name_byte(byte) => {
            let mut j = i;
            while j < bytes.len() && is_name_byte(bytes[j]) {
                j += 1;
            }
            let prefix = &source[i..j];
            if matches!(bytes.get(j), Some(b'"' | b'\'')) && is_string_prefix(prefix) {
                string(j, has_fields(prefix))
            } else {
                (Kind::Name, j)
            }
        }
        _ => (
            Kind::Op,
            i + source[i..].chars().next().map_or(1, char::len_utf8),
        ),
    }
}

/// End of the string literal starting at `pos`, prefix included.
pub(crate) fn string_at(source: &str, pos: usize) -> Option<usize> {
    match source[pos..].starts_with(|c: char| !c.is_whitespace()) {
        true => Some(lex(source, pos))
            .filter(|(kind, _)| *kind == Kind::Str)
            .map(|(_, end)| end),
        false => None,
    }
}

/// Tokens of `source`; whitespace, line continuations and line breaks inside
/// brackets are skipped.
pub(crate) fn tokenize(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let (mut i, mut depth) = (0, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\x0c' => {
                i += 1;
                continue;
            }
            b'\\' if bytes[i + 1..].starts_with(b"\r\n") => {
                i += 3;
                continue;
            }
            b'\\' if bytes.get(i + 1) == Some(&b'\n') => {
                i += 2;
                continue;
            }
            b'\n' if depth > 0 && !starts_statement(source, i + 1) => {
                i += 1;
                continue;
            }
            b'\n' => depth = 0,
            _ => {}
        }
        let (kind, end) = lex(source, i);
        let token_depth = match bytes[i] {
            b'(' | b'[' | b'{' if kind == Kind::Op => {
                depth += 1;
                depth - 1
            }
            b')' | b']' | b'}' if kind == Kind::Op => {
                depth = depth.saturating_sub(1);
                depth
            }
            _ => depth,
        };
        tokens.push(Token {
            kind,
            span: i..end,
            depth: token_depth,
        });
        i = end;
    }
    tokens
}

/// Spans of plain string literals (no prefix but `u`), outside comments.
pub(crate) fn string_literals(source: &str) -> Vec<(usize, usize)> {
    tokenize(source)
        .into_iter()
        .filter(|t| t.is_plain(source))
        .map(|t| (t.span.start, t.span.end))
        .collect()
}

fn is_op(source: &str, tokens: &[Token], i: usize, op: &str) -> bool {
    tokens
        .get(i)
        .is_some_and(|t| t.kind == Kind::Op && &source[t.span.clone()] == op)
}

/// Index of the bracket closing the one at `open`.
fn closing(source: &str, tokens: &[Token], open: usize) -> Option<usize> {
    let close = match &source[tokens[open].span.clone()] {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        _ => return None,
    };
    let depth = tokens[open].depth;
    (open + 1..tokens.len())
        .find(|&i| {
            tokens[i].kind == Kind::Newline
                || (tokens[i].kind == Kind::Op && tokens[i].depth == depth)
        })
        .filter(|&i| is_op(source, tokens, i, close))
}

/// The brackets around tokens `from..=to`, on their logical line.
fn enclosing(source: &str, tokens: &[Token], from: usize, to: usize) -> Option<(usize, usize)> {
    let mut balance = 0usize;
    for i in (0..from).rev() {
        let token = &tokens[i];
        match (token.kind, &source[token.span.clone()]) {
            (Kind::Newline, _) => return None,
            (Kind::Op, ")" | "]" | "}") => balance += 1,
            (Kind::Op, "(" | "[" | "{") if balance > 0 => balance -= 1,
            (Kind::Op, "(" | "[" | "{") => {
                return closing(source, tokens, i)
                    .filter(|&close| close > to)
                    .map(|close| (i, close));
            }
            _ => {}
        }
    }
    None
}

/// Whether tokens `a` and `b` (`a < b`) are on separate lines that only brackets join.
fn needs_brackets(source: &str, tokens: &[Token], a: usize, b: usize) -> bool {
    let between = &source[tokens[a].span.end..tokens[b].span.start];
    between
        .replace("\\\r\n", "")
        .replace("\\\n", "")
        .contains('\n')
}

/// Implicitly concatenated string literals, as token indices: runs of strings
/// with only comments between, on one logical line.
pub(crate) fn string_groups(source: &str, tokens: &[Token]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut open = false;
    for (i, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Str if open => {
                let group = groups.last_mut().unwrap();
                let last = group[group.len() - 1];
                if needs_brackets(source, tokens, last, i)
                    && enclosing(source, tokens, group[0], i).is_none()
                {
                    groups.push(vec![i]);
                } else {
                    group.push(i);
                }
            }
            Kind::Str => {
                groups.push(vec![i]);
                open = true;
            }
            Kind::Comment => {}
            _ => open = false,
        }
    }
    groups
}

/// The concatenated content of string literals.
pub(crate) fn content(source: &str, spans: &[Range<usize>]) -> String {
    spans
        .iter()
        .map(|span| {
            let literal = &source[span.clone()];
            let quoted = literal.trim_start_matches(|c: char| c.is_ascii_alphabetic());
            crate::rewriter::extract_string_content(quoted)
        })
        .collect()
}

/// A `{wrapper}[T](...)` call.
pub(crate) struct Call {
    pub span: Range<usize>,
    /// `T`, between the brackets.
    pub annotation: Range<usize>,
    /// The `(` opening the argument.
    pub open: usize,
    /// Whether the name is the wrapper itself, not `mod.{wrapper}`.
    pub exact: bool,
    /// The argument's string literals.
    pub strings: Vec<Range<usize>>,
    /// Whether the argument is plain string literals only, with comments and a
    /// trailing comma.
    pub plain: bool,
    /// The trailing comma.
    pub comma: Option<Range<usize>>,
    /// Whether the literals can stand in for the call: no comment outside them,
    /// and no line break that only the call's parentheses allow.
    pub bare: bool,
}

impl Call {
    /// The argument's string content, concatenated, when it is plain strings only.
    pub(crate) fn content(&self, source: &str) -> Option<String> {
        self.plain.then(|| content(source, &self.strings))
    }

    /// Whether the call starts at one of `any_calls`, the byte offsets of the calls
    /// whose `T` is `typing.Any`: an explicit opt-out of checks. `any_calls` is sorted.
    pub(crate) fn is_any(&self, any_calls: &[usize]) -> bool {
        any_calls.binary_search(&self.span.start).is_ok()
    }

    /// The source of the argument's literals, from the first to the last.
    pub(crate) fn argument<'a>(&self, source: &'a str) -> &'a str {
        &source[self.strings[0].start..self.strings[self.strings.len() - 1].end]
    }
}

/// The calls of a source, in source order, with an index for "which call holds this span".
pub(crate) struct Calls {
    calls: Vec<Call>,
    /// `reach[i]` is the furthest end of `calls[..=i]`; it never decreases.
    reach: Vec<usize>,
}

impl Calls {
    fn new(calls: Vec<Call>) -> Self {
        let reach = calls
            .iter()
            .scan(0, |furthest, call| {
                *furthest = (*furthest).max(call.span.end);
                Some(*furthest)
            })
            .collect();
        Self { calls, reach }
    }

    /// The first call (in source order) that holds `start..end`, in `O(log n)`.
    ///
    /// Calls start in ascending order, so the first call reaching `end` is the only
    /// candidate: any later call starts after it, and so after `start` if it does.
    pub(crate) fn enclosing(&self, start: usize, end: usize) -> Option<&Call> {
        let first = self.reach.partition_point(|&reach| reach < end);
        self.calls
            .get(first)
            .filter(|call| call.span.start <= start)
    }
}

impl IntoIterator for Calls {
    type Item = Call;
    type IntoIter = std::vec::IntoIter<Call>;

    fn into_iter(self) -> Self::IntoIter {
        self.calls.into_iter()
    }
}

impl<'a> IntoIterator for &'a Calls {
    type Item = &'a Call;
    type IntoIter = std::slice::Iter<'a, Call>;

    fn into_iter(self) -> Self::IntoIter {
        self.calls.iter()
    }
}

impl std::ops::Deref for Calls {
    type Target = [Call];

    fn deref(&self) -> &[Call] {
        &self.calls
    }
}

/// Every `{wrapper}[T](...)` and `mod.{wrapper}[T](...)` call in `source`, in source order.
pub(crate) fn wrapper_calls(source: &str, tokens: &[Token], wrapper: &str) -> Calls {
    let dotted = format!(".{wrapper}");
    let mut calls = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let name = &source[token.span.clone()];
        if token.kind != Kind::Name
            || !(name == wrapper || name.ends_with(&dotted))
            || !is_op(source, tokens, i + 1, "[")
        {
            continue;
        }
        let Some(close_bracket) = closing(source, tokens, i + 1) else {
            continue;
        };
        let open = close_bracket + 1;
        let Some(close) = Some(open)
            .filter(|&j| is_op(source, tokens, j, "("))
            .and_then(|j| closing(source, tokens, j))
        else {
            continue;
        };
        let mut args = open + 1..close;
        let mut comma = None;
        if let Some(last) = tokens[args.clone()]
            .iter()
            .rposition(|t| t.kind != Kind::Comment)
        {
            if is_op(source, tokens, open + 1 + last, ",") {
                comma = Some(tokens[open + 1 + last].span.clone());
                args = open + 1..open + 1 + last;
            }
        }
        let code: Vec<usize> = args
            .clone()
            .filter(|&j| tokens[j].kind != Kind::Comment)
            .collect();
        let strings: Vec<Range<usize>> = code
            .iter()
            .filter(|&&j| tokens[j].kind == Kind::Str)
            .map(|&j| tokens[j].span.clone())
            .collect();
        let plain = !code.is_empty() && code.iter().all(|&j| tokens[j].is_plain(source));
        let bare = plain && {
            let (first, last) = (code[0], code[code.len() - 1]);
            let outside = (open + 1..close)
                .any(|j| tokens[j].kind == Kind::Comment && (j < first || j > last));
            let split = code
                .windows(2)
                .any(|w| needs_brackets(source, tokens, w[0], w[1]));
            !outside && (!split || enclosing(source, tokens, i, close).is_some())
        };
        calls.push(Call {
            span: token.span.start..tokens[close].span.end,
            annotation: tokens[i + 1].span.end..tokens[close_bracket].span.start,
            open: tokens[open].span.start,
            exact: name == wrapper,
            strings,
            plain,
            comma,
            bare,
        });
    }
    Calls::new(calls)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calls(source: &str) -> Calls {
        wrapper_calls(source, &tokenize(source), "SQL")
    }

    #[test]
    fn test_enclosing_finds_the_outermost_call_holding_a_span() {
        let source = "SQL[A](\"a\")\nSQL[B](SQL[C](\"c\"))\n";
        let found = calls(source);
        let inner = source.find("SQL[C]").unwrap();
        let outer = source.find("SQL[B]").unwrap();
        let hit = |start: usize, end: usize| found.enclosing(start, end).map(|c| c.span.start);
        assert_eq!(hit(inner, inner + 3), Some(outer));
        assert_eq!(hit(source.find("\"a\"").unwrap(), 11), Some(0));
        assert_eq!(hit(source.len() - 1, source.len()), None);
        assert_eq!(hit(outer - 1, outer + 2), None);
    }

    fn texts(source: &str) -> Vec<(Kind, &str)> {
        tokenize(source)
            .into_iter()
            .map(|t| (t.kind, &source[t.span]))
            .collect()
    }

    #[test]
    fn test_string_literals_skip_past_prefixed_strings() {
        let source = "# it's\nf = f\"a\" + br'\\'\"' + 'it\\'s' + u\"c\"\nh = \"b\"\n";
        let spans: Vec<&str> = string_literals(source)
            .into_iter()
            .map(|(start, end)| &source[start..end])
            .collect();
        assert_eq!(spans, vec!["'it\\'s'", "u\"c\"", "\"b\""]);
    }

    #[test]
    fn test_keyword_then_string() {
        for source in ["return\"x\"", "if\"(\"", "else'a'", "in\"((\""] {
            let kinds: Vec<Kind> = texts(source).into_iter().map(|(k, _)| k).collect();
            assert_eq!(kinds, vec![Kind::Name, Kind::Str], "{source}");
        }
    }

    #[test]
    fn test_fstring_fields() {
        for source in [
            "f\"{\"(\"}\"",
            "f'{x:>{w}}'",
            "f\"{d['(']!r}\"",
            "t\"{f'{\"[\"}'}\"",
            "f\"{{(\"",
            "f\"\"\"{x  # (\n}\"\"\"",
            "f\"\"\"{x:\n}\"\"\"",
        ] {
            assert_eq!(texts(source), vec![(Kind::Str, source)], "{source}");
        }
    }

    #[test]
    fn test_strings() {
        for source in [
            "\"\"\"a \" ) '' \"\"\"",
            "'''x'' ' '''",
            "\"a\\\\\"",
            "'a\\'('",
            "\"a\\\r\nb\"",
        ] {
            assert_eq!(texts(source), vec![(Kind::Str, source)], "{source:?}");
        }
        assert_eq!(
            texts("x = \"a(\ny = 1"),
            vec![
                (Kind::Name, "x"),
                (Kind::Op, "="),
                (Kind::Error, "\"a("),
                (Kind::Newline, "\n"),
                (Kind::Name, "y"),
                (Kind::Op, "="),
                (Kind::Name, "1"),
            ]
        );
    }

    #[test]
    fn test_crlf_continuation() {
        let kinds: Vec<Kind> = texts("q = \"a\" \\\r\n    \"b\"\r\n")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            kinds,
            vec![Kind::Name, Kind::Op, Kind::Str, Kind::Str, Kind::Newline]
        );
    }

    #[test]
    fn test_depth_resyncs_at_statement() {
        let source = "x = (\ndef f(): pass\nq = (\"a\"\n \"b\")\n";
        let tokens = tokenize(source);
        assert_eq!(string_groups(source, &tokens).len(), 1);
        assert!(tokens
            .iter()
            .any(|t| t.kind == Kind::Newline && t.span.start == 5));
    }

    #[test]
    fn test_matmul_at_line_start_keeps_depth() {
        let source = "x = (a\n@ b, \"q \"\n\"n\")\n";
        let tokens = tokenize(source);
        let groups: Vec<usize> = string_groups(source, &tokens)
            .iter()
            .map(Vec::len)
            .collect();
        assert_eq!(groups, vec![2]);
    }

    #[test]
    fn test_call_spans_concatenation_and_comments() {
        let source = "q = SQL[tuple[int]](\"a \\\"b\\\" \"  # c\n  'd'  # e\n)\nr = SQL[T](x)";
        let found = calls(source);
        assert_eq!(found.len(), 2);
        assert_eq!(&source[found[0].annotation.clone()], "tuple[int]");
        assert_eq!(found[0].content(source).as_deref(), Some("a \\\"b\\\" d"));
        assert!(!found[0].bare);
        assert_eq!(&source[found[1].span.clone()], "SQL[T](x)");
        assert!(!found[1].plain);
    }

    #[test]
    fn test_calls() {
        let source = "a, b = SQL[T](f(\"(\", (1))), mod.SQL[U](\"x\")";
        let found = calls(source);
        assert_eq!(found.len(), 2);
        assert_eq!(&source[found[0].span.clone()], "SQL[T](f(\"(\", (1)))");
        assert!(found[0].exact && !found[0].plain);
        assert_eq!(&source[found[1].span.clone()], "mod.SQL[U](\"x\")");
        assert!(!found[1].exact && found[1].plain);
    }

    #[test]
    fn test_bare() {
        for (source, expected) in [
            ("q = SQL[T](\"a\",)", true),
            ("q = SQL[T](\"a\" \"b\")", true),
            ("q = SQL[T](\"a\" \\\n \"b\")", true),
            ("q = SQL[T](\"a\"\n \"b\")", false),
            ("q = (SQL[T](\"a\"\n \"b\"))", true),
            ("q = (SQL[T](  # c\n\"a\"))", false),
            ("q = SQL[T](f\"a\")", false),
            ("q = SQL[T](\"a\",  # c\n)", false),
            ("if\"(\": pass\nq = SQL[T](\n \"a\"\n \"b\"\n)", false),
        ] {
            let found = calls(source);
            assert_eq!(found[0].bare, expected, "{source}");
        }
    }

    #[test]
    fn test_groups_end_at_logical_lines() {
        let source = "a = (\"x\"  # c\n \"y\")\nb = \"z\"\n\"w\"\nx = f\"{\"(\"}\"\nq = \"v\"\n\"\"\"doc\"\"\"";
        let tokens = tokenize(source);
        let groups: Vec<usize> = string_groups(source, &tokens)
            .iter()
            .map(Vec::len)
            .collect();
        assert_eq!(groups, vec![2, 1, 1, 1, 1, 1]);
    }
}
