//! `# sqlty: ignore` comments, and the diagnostics they suppress
//! (`ParsedSource.ignores`, `apply_suppressions`).

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use ruff_python_ast::token::TokenKind;
use ruff_text_size::Ranged;

use crate::checker::Diagnostic;
use crate::rows::Reported;
use crate::rules::{Rule, RuleConfig};
use crate::types::generic::is_word;
use crate::types::symbols::Source;

/// A `# sqlty: ignore` or `# sqlty: ignore[rule, ...]` comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ignore {
    /// The 1-based line it is on, whose diagnostics it suppresses.
    pub line: usize,
    /// The rules it names; `None` suppresses every rule.
    pub rules: Option<Vec<String>>,
    /// The byte offsets of the comment.
    pub span: (usize, usize),
}

fn blank(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// The ignore a comment spells, as `#\s*sqlty:\s*ignore\b(?:\[([^\]]*)\])?` matches it.
fn rules_of(comment: &str) -> Option<Option<Vec<String>>> {
    static IGNORE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^#[\s\x1c-\x1f]*sqlty:[\s\x1c-\x1f]*ignore(?:\[([^\]]*)\])?")
            .expect("a valid pattern")
    });
    let found = IGNORE.captures(comment)?;
    let Some(names) = found.get(1) else {
        // `ignore\b`: `ignored` is another word, `ignore[` with no `]` still ignores.
        let rest = &comment[found.get(0)?.end()..];
        return (!rest.chars().next().is_some_and(is_word)).then_some(None);
    };
    Some(Some(
        names
            .as_str()
            .split(',')
            .map(|name| name.trim_matches(blank))
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect(),
    ))
}

/// The ignore comments of `source`, in order; a line has at most one.
pub fn ignores(source: &Source) -> Vec<Ignore> {
    let text = &*source.text;
    if !text.contains("sqlty:") {
        return Vec::new();
    }
    source
        .parsed
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .filter_map(|token| {
            let range = token.range();
            Some(Ignore {
                line: source.line(range.start()),
                rules: rules_of(&text[range])?,
                span: (range.start().to_usize(), range.end().to_usize()),
            })
        })
        .collect()
}

/// `apply_suppressions`: `diagnostics` without those a comment on their line suppresses,
/// then an `unused-ignore` for each comment, or rule of one, that suppresses nothing.
pub fn apply(ignores: &[Ignore], diagnostics: Vec<Reported>, rules: &RuleConfig) -> Vec<Reported> {
    if ignores.is_empty() {
        return diagnostics;
    }
    let mut kept = Vec::new();
    let mut used: HashSet<(usize, Option<&str>)> = HashSet::new();
    for reported in diagnostics {
        let diagnostic = &reported.diagnostic;
        let rule = diagnostic.rule.name();
        let ignore = ignores.iter().find(|ignore| ignore.line == diagnostic.line);
        match ignore {
            Some(Ignore {
                line, rules: None, ..
            }) => {
                used.insert((*line, None));
            }
            Some(Ignore {
                line,
                rules: Some(names),
                ..
            }) if names.iter().any(|name| name == rule) => {
                used.insert((*line, Some(rule)));
            }
            _ => kept.push(reported),
        }
    }
    if rules.is_enabled(Rule::UnusedIgnore) {
        let severity = rules.severity(Rule::UnusedIgnore);
        for ignore in ignores {
            let unused: Vec<Option<&str>> = match &ignore.rules {
                None => (!used.contains(&(ignore.line, None)))
                    .then_some(None)
                    .into_iter()
                    .collect(),
                Some(names) => names
                    .iter()
                    .map(|name| Some(name.as_str()))
                    .filter(|name| !used.contains(&(ignore.line, *name)))
                    .collect(),
            };
            for name in unused {
                let written = match name {
                    None => "# sqlty: ignore".to_string(),
                    Some(name) => format!("# sqlty: ignore[{name}]"),
                };
                kept.push(Reported {
                    diagnostic: Diagnostic {
                        rule: Rule::UnusedIgnore,
                        message: format!("'{written}' suppresses no diagnostic"),
                        severity,
                        line: ignore.line,
                        span: ignore.span.0..ignore.span.1,
                        fix: Vec::new(),
                    },
                    redundant: None,
                });
            }
        }
    }
    kept
}
