//! One lexing pass over a source, shared by check, rewrite, strip and fix.
//!
//! [`Scan`] holds what every pass asks of a source: its tokens, the `{wrapper}[T](...)`
//! calls, the string groups and the line index. Building it is the only place that
//! tokenizes, so a pass that needs several of these (check needs all of them) lexes once.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::ops::Range;

use crate::rewriter::{after_marker, decode_escapes, sql_names, LineIndex};
use crate::rules::Rule;
use crate::template::template_content;
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
    /// The names sqlty's `SQL` is bound to (see [`sql_names`]).
    pub sql_names: Vec<String>,
    groups: OnceCell<Vec<Group>>,
}

impl<'s> Scan<'s> {
    /// Tokenize `source`, finding the calls of `wrapper` (`SQL`, `sqlty.SQL`, ...).
    pub(crate) fn new(source: impl Into<Cow<'s, str>>, wrapper: &str) -> Self {
        let text = source.into();
        let tokens = tokenize(&text);
        let sql_names = sql_names(&text, &tokens);
        let calls = wrapper_calls(&text, &tokens, wrapper, &sql_names);
        let lines = LineIndex::new(&text);
        Self {
            text,
            tokens,
            calls,
            lines,
            sql_names,
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

    /// Whether every literal of `group` is a t-string of plain value fields, which is
    /// checked and rewritten as a `SQLTemplate[T]` (see `tokens::template_content`).
    pub(crate) fn is_template(&self, group: &[usize]) -> bool {
        group
            .iter()
            .all(|&i| self.tokens[i].is_template(&self.text))
    }

    /// The first field of `group`'s t-strings that keeps it from being read as a query.
    pub(crate) fn template_blocker(&self, group: &[usize]) -> Option<String> {
        group.iter().find_map(|&i| {
            let token = &self.tokens[i];
            (token.prefix(&self.text) == "t")
                .then(|| template_content(&self.text[token.span.clone()], &self.text).err())
                .flatten()
                .filter(|field| !field.is_empty())
        })
    }

    /// The strings `check` reads as a query by their marker, as `(line, literal spans,
    /// wrapper)`: a marked group outside any wrapper call, or the marked plain argument of a
    /// `{wrapper}[T](...)` call. In source order of groups, then of calls.
    pub(crate) fn marked_strings(&self) -> Vec<(usize, Vec<Range<usize>>, bool)> {
        let marked = |content: &str| after_marker(&decode_escapes(content)).is_some();
        let groups = self
            .groups()
            .iter()
            .filter(|group| group.skipped.is_none() && self.enclosing_call(group).is_none())
            .filter(|group| marked(&self.content(group)))
            .map(|group| (group.line, self.spans(group), false));
        let calls = self
            .calls
            .iter()
            .filter(|call| call.content(&self.text).is_some_and(|text| marked(&text)))
            .map(|call| {
                (
                    self.lines.line_at(call.span.start),
                    call.strings.clone(),
                    true,
                )
            });
        groups.chain(calls).collect()
    }

    /// Spans of plain string literals (no prefix but `u`), outside comments.
    pub(crate) fn plain_literals(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.tokens
            .iter()
            .filter(|t| t.is_plain(&self.text))
            .map(|t| (t.span.start, t.span.end))
    }

    /// Why rewrite leaves a string group alone although it holds a `--sql` string:
    /// `fstring-skipped`, `tstring-skipped` (a t-string a field of which changes the SQL),
    /// or `unchecked-query` for a raw or bytes part.
    fn skipped_rule(&self, group: &[usize]) -> Option<Rule> {
        let prefixes: String = group
            .iter()
            .map(|&i| self.tokens[i].prefix(&self.text))
            .collect();
        let spans: Vec<Range<usize>> = group.iter().map(|&i| self.tokens[i].span.clone()).collect();
        if prefixes.trim_matches('u').is_empty()
            || self.is_template(group)
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

    /// Whether the string at token `first` is in the pattern of a `case` clause, which
    /// takes literals and no calls (its `if` guard is an expression).
    pub(crate) fn in_case_pattern(&self, first: usize) -> bool {
        let text = |i: usize| &self.text[self.tokens[i].span.clone()];
        let start = (0..first)
            .rev()
            .find(|&i| self.tokens[i].kind == Kind::Newline && self.tokens[i].depth == 0)
            .map_or(0, |i| i + 1);
        if start >= first
            || self.tokens[start].kind != Kind::Name
            || text(start) != "case"
            // `case = ...`, `case: int = ...`: a name, not the soft keyword.
            || (self.tokens[start + 1].kind == Kind::Op
                && !matches!(text(start + 1), "(" | "[" | "{" | "-"))
        {
            return false;
        }
        !(start + 1..first).any(|i| {
            self.tokens[i].depth == 0
                && matches!(
                    (&self.tokens[i].kind, text(i)),
                    (Kind::Op, ":") | (Kind::Name, "if")
                )
        })
    }

    /// The span of `group`'s call, and the end of its name, when `group` is the only
    /// argument of an untyped `SQL(...)` named one of [`Scan::sql_names`].
    pub(crate) fn bare_call(&self, group: &Group) -> Option<(Range<usize>, usize)> {
        let (first, last) = (group.tokens[0], group.tokens[group.tokens.len() - 1]);
        let text = |i: usize| &self.text[self.tokens[i].span.clone()];
        let is_op = |i: usize, op: &str| self.tokens[i].kind == Kind::Op && text(i) == op;
        let close = (last + 1..self.tokens.len())
            .find(|&i| self.tokens[i].kind != Kind::Comment && !is_op(i, ","))?;
        let name = first.checked_sub(2)?;
        let bare = is_op(first - 1, "(")
            && is_op(close, ")")
            && self.tokens[name].kind == Kind::Name
            && self.sql_names.iter().any(|n| n == text(name))
            && !(name > 0 && is_op(name - 1, "."));
        let name = &self.tokens[name].span;
        bare.then(|| (name.start..self.tokens[close].span.end, name.end))
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
