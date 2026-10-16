//! One lexing pass over a source, shared by check, rewrite, strip and fix.
//!
//! [`Scan`] holds what every pass asks of a source: its tokens, the `{wrapper}[T](...)`
//! calls, the string groups and the line index. Building it is the only place that
//! tokenizes, so a pass that needs several of these (check needs all of them) lexes once.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::ops::Range;

use crate::rewriter::{after_marker, LineIndex};
use crate::rules::Rule;
use crate::tokens::{content, string_groups, tokenize, wrapper_calls, Call, Calls, Kind, Token};

/// A string literal, or implicitly concatenated literals, and what the passes ask of it.
pub(crate) struct Group {
    /// Indices into [`Scan::tokens`].
    pub tokens: Vec<usize>,
    /// Source offsets of the first token's start and the last token's end.
    pub start: usize,
    pub end: usize,
    /// 1-indexed line of `start`.
    pub line: usize,
    /// Index into [`Scan::calls`] of the first call holding the group.
    pub enclosing: Option<usize>,
    /// Why rewrite leaves the group alone although it holds a `--sql` string.
    pub skipped: Option<Rule>,
}

pub(crate) struct Scan<'s> {
    text: Cow<'s, str>,
    pub tokens: Vec<Token>,
    pub calls: Calls,
    pub lines: LineIndex,
    groups: OnceCell<Vec<Group>>,
}

impl<'s> Scan<'s> {
    /// Tokenize `source`, finding the calls of `wrapper` (`SQL`, `sqlty.SQL`, ...).
    pub(crate) fn new(source: impl Into<Cow<'s, str>>, wrapper: &str) -> Self {
        let text = source.into();
        let tokens = tokenize(&text);
        let calls = wrapper_calls(&text, &tokens, wrapper);
        let lines = LineIndex::new(&text);
        Self {
            text,
            tokens,
            calls,
            lines,
            groups: OnceCell::new(),
        }
    }

    pub(crate) fn source(&self) -> &str {
        &self.text
    }

    /// The call `group` sits in, if any.
    pub(crate) fn enclosing_call(&self, group: &Group) -> Option<&Call> {
        group.enclosing.map(|i| &self.calls[i])
    }

    /// The string groups, in source order; found on first use.
    pub(crate) fn groups(&self) -> &[Group] {
        self.groups.get_or_init(|| {
            string_groups(&self.text, &self.tokens)
                .into_iter()
                .map(|tokens| self.group(tokens))
                .collect()
        })
    }

    fn group(&self, tokens: Vec<usize>) -> Group {
        let start = self.tokens[tokens[0]].span.start;
        let end = self.tokens[tokens[tokens.len() - 1]].span.end;
        let skipped = self.skipped_rule(&tokens);
        Group {
            line: self.lines.line_at(start),
            enclosing: self
                .calls
                .enclosing_index(start, self.tokens[tokens[0]].span.end),
            tokens,
            start,
            end,
            skipped,
        }
    }

    /// Spans of the literals of `group`.
    pub(crate) fn spans(&self, group: &Group) -> Vec<Range<usize>> {
        group
            .tokens
            .iter()
            .map(|&i| self.tokens[i].span.clone())
            .collect()
    }

    /// The concatenated content of the literals of `group`.
    pub(crate) fn content(&self, group: &Group) -> String {
        content(&self.text, &self.spans(group))
    }

    /// Whether every literal of `group` has no prefix but `u`.
    pub(crate) fn is_plain(&self, group: &Group) -> bool {
        group
            .tokens
            .iter()
            .all(|&i| self.tokens[i].is_plain(&self.text))
    }

    /// Spans of plain string literals (no prefix but `u`), outside comments.
    pub(crate) fn plain_literals(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.tokens
            .iter()
            .filter(|t| t.is_plain(&self.text))
            .map(|t| (t.span.start, t.span.end))
    }

    /// Why rewrite leaves a string group alone although it holds a `--sql` string:
    /// `fstring-skipped`, `tstring-skipped`, or `unchecked-query` for a raw or bytes part.
    fn skipped_rule(&self, group: &[usize]) -> Option<Rule> {
        let prefixes: String = group
            .iter()
            .map(|&i| self.tokens[i].prefix(&self.text))
            .collect();
        let spans: Vec<Range<usize>> = group.iter().map(|&i| self.tokens[i].span.clone()).collect();
        if prefixes.trim_matches('u').is_empty()
            || after_marker(&content(&self.text, &spans)).is_none()
        {
            return None;
        }
        Some(if prefixes.contains('t') {
            Rule::TstringSkipped
        } else if prefixes.contains('f') {
            Rule::FstringSkipped
        } else {
            Rule::UncheckedQuery
        })
    }

    /// Whether the string at token `first` opens the argument of a call named `SQL`
    /// or ending in it (`sql.SQL(`, `PgSQL(`): a psycopg query, not for a wrapper.
    pub(crate) fn in_composed_sql(&self, first: usize) -> bool {
        let call = |i: usize| {
            self.tokens[i].kind == Kind::Name
                && self.text[self.tokens[i].span.clone()].ends_with("SQL")
        };
        first >= 2
            && self.tokens[first - 1].kind == Kind::Op
            && &self.text[self.tokens[first - 1].span.clone()] == "("
            && call(first - 2)
    }
}
