//! Just enough of a Python tokenizer to find string literals, comments and
//! `{wrapper}[T](...)` calls, so edits never cut a call or a string in two.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A string literal; `plain` when it has no prefix but `u`.
    Str {
        plain: bool,
    },
    Comment,
    /// A dotted name, keyword or number.
    Name,
    /// The end of a logical line.
    Newline,
    Op,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
    /// Bracket depth; a bracket and its match share it.
    pub depth: usize,
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' || byte >= 0x80
}

fn is_string_prefix(prefix: &str) -> bool {
    matches!(
        prefix.to_ascii_lowercase().as_str(),
        "" | "u" | "r" | "b" | "f" | "t" | "br" | "rb" | "fr" | "rf" | "tr" | "rt"
    )
}

/// End of the string literal whose opening quote is at `at`, if it is closed.
fn string_end(bytes: &[u8], at: usize) -> Option<usize> {
    let quote = bytes[at];
    let triple = bytes[at..].starts_with(&[quote; 3]);
    let mut i = at + if triple { 3 } else { 1 };
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' if !triple => return None,
            byte if byte == quote && (!triple || bytes[i..].starts_with(&[quote; 3])) => {
                return Some(i + if triple { 3 } else { 1 });
            }
            _ => i += 1,
        }
    }
    None
}

/// Tokens of `source`; whitespace, line continuations and line breaks inside
/// brackets are skipped.
pub(crate) fn tokenize(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let (mut i, mut depth) = (0, 0usize);
    while i < bytes.len() {
        let start = i;
        let (kind, end) = match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\x0c' => {
                i += 1;
                continue;
            }
            b'\\' if matches!(bytes.get(i + 1), Some(b'\n' | b'\r')) => {
                i += 2;
                continue;
            }
            b'\n' if depth > 0 => {
                i += 1;
                continue;
            }
            b'\n' => (Kind::Newline, i + 1),
            b'#' => (
                Kind::Comment,
                source[i..].find('\n').map_or(bytes.len(), |n| i + n),
            ),
            b'"' | b'\'' => match string_end(bytes, i) {
                Some(end) => (Kind::Str { plain: true }, end),
                None => (Kind::Op, i + 1),
            },
            byte if is_name_byte(byte) => {
                let mut j = i;
                while j < bytes.len() && is_name_byte(bytes[j]) {
                    j += 1;
                }
                let prefix = &source[i..j];
                match bytes.get(j) {
                    Some(b'"' | b'\'') if is_string_prefix(prefix) => match string_end(bytes, j) {
                        Some(end) => (
                            Kind::Str {
                                plain: prefix.eq_ignore_ascii_case("u"),
                            },
                            end,
                        ),
                        None => (Kind::Op, j + 1),
                    },
                    // A quote after a word that is no prefix (`it's`) opens nothing.
                    Some(b'"' | b'\'') => (Kind::Name, j + 1),
                    _ => (Kind::Name, j),
                }
            }
            _ => (Kind::Op, i + 1),
        };
        let token_depth = match bytes[start] {
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
            span: start..end,
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
        .filter(|t| t.kind == Kind::Str { plain: true })
        .map(|t| (t.span.start, t.span.end))
        .collect()
}

/// Implicitly concatenated string literals: runs of strings with only comments between.
pub(crate) fn string_groups(tokens: &[Token]) -> Vec<Vec<&Token>> {
    let mut groups: Vec<Vec<&Token>> = Vec::new();
    let mut open = false;
    for token in tokens {
        match token.kind {
            Kind::Str { .. } if open => groups.last_mut().unwrap().push(token),
            Kind::Str { .. } => {
                groups.push(vec![token]);
                open = true;
            }
            Kind::Comment => {}
            _ => open = false,
        }
    }
    groups
}

/// A `{wrapper}[T](...)` call.
pub(crate) struct Call {
    pub span: Range<usize>,
    /// `T`, between the brackets.
    pub annotation: Range<usize>,
    /// The argument's string literals, when it is plain ones only (with comments
    /// and a trailing comma).
    pub literals: Option<Vec<Range<usize>>>,
    /// Whether the literals can replace the whole call: no comment outside them,
    /// and they need no brackets around them.
    pub strippable: bool,
}

impl Call {
    /// The argument's string content, concatenated.
    pub fn content(&self, source: &str) -> Option<String> {
        let literals = self.literals.as_ref()?;
        Some(
            literals
                .iter()
                .map(|span| crate::rewriter::extract_string_content(&source[span.clone()]))
                .collect(),
        )
    }

    /// The source of the argument's literals, from the first to the last.
    pub fn argument<'a>(&self, source: &'a str) -> Option<&'a str> {
        let literals = self.literals.as_ref()?;
        Some(&source[literals.first()?.start..literals.last()?.end])
    }
}

/// Index of the bracket closing the one at `open`.
fn closing(tokens: &[Token], open: usize) -> Option<usize> {
    let depth = tokens[open].depth;
    (open + 1..tokens.len()).find(|&i| tokens[i].kind == Kind::Op && tokens[i].depth == depth)
}

/// Every `{wrapper}[T](...)` call in `source`, in source order.
pub(crate) fn wrapper_calls(source: &str, tokens: &[Token], wrapper: &str) -> Vec<Call> {
    let is_op = |i: usize, op: &str| {
        tokens
            .get(i)
            .is_some_and(|t| t.kind == Kind::Op && &source[t.span.clone()] == op)
    };
    let mut calls = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Name || &source[token.span.clone()] != wrapper || !is_op(i + 1, "[")
        {
            continue;
        }
        let Some(close_bracket) = closing(tokens, i + 1).filter(|&j| is_op(j, "]")) else {
            continue;
        };
        let open = close_bracket + 1;
        let Some(close) = Some(open)
            .filter(|&j| is_op(j, "("))
            .and_then(|j| closing(tokens, j))
            .filter(|&j| is_op(j, ")"))
        else {
            continue;
        };
        let mut args = &tokens[open + 1..close];
        if let Some(last) = args.iter().rposition(|t| t.kind != Kind::Comment) {
            if is_op(open + 1 + last, ",") {
                args = &tokens[open + 1..open + 1 + last];
            }
        }
        let strings: Vec<&Token> = args.iter().filter(|t| t.kind != Kind::Comment).collect();
        let plain =
            !strings.is_empty() && strings.iter().all(|t| t.kind == Kind::Str { plain: true });
        let literals: Option<Vec<Range<usize>>> =
            plain.then(|| strings.iter().map(|t| t.span.clone()).collect());
        let strippable = literals.as_ref().is_some_and(|literals| {
            let (first, last) = (&literals[0], &literals[literals.len() - 1]);
            let outside = args.iter().any(|t| {
                t.kind == Kind::Comment && (t.span.end <= first.start || t.span.start >= last.end)
            });
            let between = literals.windows(2).any(|w| {
                source[w[0].end..w[1].start]
                    .replace("\\\r\n", "")
                    .replace("\\\n", "")
                    .contains('\n')
            });
            !outside && (token.depth > 0 || !between)
        });
        calls.push(Call {
            span: token.span.start..tokens[close].span.end,
            annotation: tokens[i + 1].span.end..tokens[close_bracket].span.start,
            literals,
            strippable,
        });
    }
    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calls(source: &str) -> Vec<Call> {
        wrapper_calls(source, &tokenize(source), "SQL")
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
    fn test_call_spans_concatenation_and_comments() {
        let source = "q = SQL[tuple[int]](\"a \\\"b\\\" \"  # c\n  'd'  # e\n)\nr = SQL[T](x)";
        let found = calls(source);
        assert_eq!(found.len(), 2);
        assert_eq!(&source[found[0].annotation.clone()], "tuple[int]");
        assert_eq!(found[0].content(source).as_deref(), Some("a \\\"b\\\" d"));
        assert!(!found[0].strippable);
        assert_eq!(&source[found[1].span.clone()], "SQL[T](x)");
        assert!(found[1].literals.is_none());
    }

    #[test]
    fn test_strippable() {
        for (source, expected) in [
            ("q = SQL[T](\"a\",)", true),
            ("q = SQL[T](\"a\" \"b\")", true),
            ("q = SQL[T](\"a\" \\\n \"b\")", true),
            ("q = SQL[T](\"a\"\n \"b\")", false),
            ("q = (SQL[T](\"a\"\n \"b\"))", true),
            ("q = (SQL[T](  # c\n\"a\"))", false),
            ("q = SQL[T](f\"a\")", false),
        ] {
            assert_eq!(calls(source)[0].strippable, expected, "{source}");
        }
    }

    #[test]
    fn test_groups_end_at_logical_lines() {
        let source = "a = (\"x\"  # c\n \"y\")\nb = \"z\"\n\"w\"";
        let tokens = tokenize(source);
        let groups: Vec<usize> = string_groups(&tokens).iter().map(Vec::len).collect();
        assert_eq!(groups, vec![2, 1, 1]);
    }
}
