//! `# sqlty: ignore` comments, and the diagnostics they suppress; `-- sqlty: ignore` in
//! `.sql` files.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use ruff_python_ast::token::TokenKind;
use ruff_python_parser::{lexer, Mode};

use crate::checker::Diagnostic;
use crate::rows::Reported;
use crate::rules::{Rule, RuleConfig};
use crate::scopes::Lines;
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
        sqlty_sql::regex(r"^#[\s\x1c-\x1f]*sqlty:[\s\x1c-\x1f]*ignore(?:\[([^\]]*)\])?")
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
        .comments
        .iter()
        .filter_map(|&range| {
            Some(Ignore {
                line: source.line(range.start()),
                rules: rules_of(&text[range])?,
                span: (range.start().to_usize(), range.end().to_usize()),
            })
        })
        .collect()
}

/// The ignore comments of a source that does not parse, in order: every comment the lexer
/// finds, past what does not lex, so an edit that breaks one line keeps the others.
pub fn unparsed_ignores(text: &str) -> Vec<Ignore> {
    if !text.contains("sqlty:") {
        return Vec::new();
    }
    let lines = Lines::new(text);
    let mut tokens = lexer::lex(text, Mode::Module);
    let mut found: Vec<Ignore> = Vec::new();
    loop {
        match tokens.next_token() {
            TokenKind::EndOfFile => return found,
            TokenKind::Comment => {
                let range = tokens.current_range();
                if let Some(rules) = rules_of(&text[range]) {
                    found.push(Ignore {
                        line: lines.line(range.start().to_usize()),
                        rules,
                        span: (range.start().to_usize(), range.end().to_usize()),
                    });
                }
            }
            _ => {}
        }
    }
}

/// The `-- sqlty: ignore` comments of a `.sql` file, the first of each line.
pub fn sql_ignores(text: &str) -> Vec<Ignore> {
    #[expect(clippy::expect_used, reason = "the pattern is written here")]
    static IGNORE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
        regex::bytes::Regex::new(r"(?-u)--\s*sqlty:\s*ignore\b(?:\[([^\]]*)\])?")
            .expect("a valid pattern")
    });
    if !text.contains("sqlty:") {
        return Vec::new();
    }
    let lines = Lines::new(text);
    let mut found = Vec::new();
    for line in 1.. {
        let start = lines.start(line, text.len());
        if start == text.len() {
            break;
        }
        let end = lines.start(line + 1, text.len());
        let Some(matched) = IGNORE.captures(&text.as_bytes()[start..end]) else {
            continue;
        };
        let whole = matched.get_match();
        found.push(Ignore {
            line,
            rules: matched.get(1).map(|names| {
                String::from_utf8_lossy(names.as_bytes())
                    .split(',')
                    .map(|name| name.trim_matches(blank))
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .collect()
            }),
            span: (start + whole.start(), start + whole.end()),
        });
    }
    found
}

/// What `fix_source` leaves alone, by line: the rules of each ignore comment (`None`: every
/// rule), and `stale-rewrite` on each line of a helper call whose strings need no `SQL[T]`.
pub fn suppressed(
    ignores: &[Ignore],
    typed: impl IntoIterator<Item = (usize, usize)>,
) -> crate::rewriter::Suppressed {
    let mut found: crate::rewriter::Suppressed = ignores
        .iter()
        .map(|ignore| (ignore.line, ignore.rules.clone()))
        .collect();
    for (first, last) in typed {
        for line in first..=last {
            if let Some(named) = found
                .entry(line)
                .or_insert_with(|| Some(Vec::new()))
                .as_mut()
            {
                named.push(Rule::StaleRewrite.name().to_string());
            }
        }
    }
    found
}

/// `apply_suppressions`: `diagnostics` without those a comment on their line suppresses,
/// then an `unused-ignore` for each comment, or rule of one, that suppresses nothing.
pub fn apply(ignores: &[Ignore], diagnostics: Vec<Reported>, rules: &RuleConfig) -> Vec<Reported> {
    apply_marked(ignores, diagnostics, rules, "#")
}

/// [`apply`] for a `.sql` file, whose comments start with `--`.
pub fn apply_sql(
    ignores: &[Ignore],
    diagnostics: Vec<Diagnostic>,
    rules: &RuleConfig,
) -> Vec<Diagnostic> {
    let reported = diagnostics
        .into_iter()
        .map(|diagnostic| Reported {
            diagnostic,
            redundant: None,
        })
        .collect();
    apply_marked(ignores, reported, rules, "--")
        .into_iter()
        .map(|reported| reported.diagnostic)
        .collect()
}

fn apply_marked(
    ignores: &[Ignore],
    diagnostics: Vec<Reported>,
    rules: &RuleConfig,
    marker: &str,
) -> Vec<Reported> {
    let crashed = |reported: &Reported| reported.diagnostic.rule == Rule::InternalError;
    if ignores.is_empty() || diagnostics.iter().any(crashed) {
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
                    None => format!("{marker} sqlty: ignore"),
                    Some(name) => format!("{marker} sqlty: ignore[{name}]"),
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn a_named_ignore_suppresses_its_rule_without_unused_ignore() {
        let source = Source::new("q = 1  # sqlty: ignore[select-star]\n", (3, 14)).unwrap();
        let reported = Reported {
            diagnostic: Diagnostic {
                rule: Rule::SelectStar,
                message: String::new(),
                severity: crate::rules::Severity::Error,
                line: 1,
                span: 0..1,
                fix: Vec::new(),
            },
            redundant: None,
        };
        let rules = RuleConfig::default().only(&[Rule::SelectStar]);
        assert!(apply(&ignores(&source), vec![reported], &rules).is_empty());
    }

    #[test]
    fn a_crashed_module_reports_no_unused_ignore() {
        let source = Source::new("q = 1  # sqlty: ignore[select-star]\n", (3, 14)).unwrap();
        let reported = Reported {
            diagnostic: Diagnostic {
                rule: Rule::InternalError,
                message: String::new(),
                severity: crate::rules::Severity::Error,
                line: 1,
                span: 0..0,
                fix: Vec::new(),
            },
            redundant: None,
        };
        let rules = RuleConfig::default().only(&[Rule::InternalError, Rule::UnusedIgnore]);
        let kept = apply(&ignores(&source), vec![reported], &rules);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].diagnostic.rule, Rule::InternalError);
    }

    fn lines_and_rules(found: &[Ignore]) -> Vec<(usize, Option<Vec<&str>>)> {
        found
            .iter()
            .map(|ignore| {
                let rules = ignore
                    .rules
                    .as_ref()
                    .map(|rules| rules.iter().map(String::as_str).collect());
                (ignore.line, rules)
            })
            .collect()
    }

    #[test]
    fn a_source_that_does_not_parse_keeps_every_comment_the_lexer_finds() {
        let text =
            "# note\nx = (  # sqlty: ignore\ny = $  # sqlty: ignore[a]\nz = '# sqlty: ignore\n";
        assert!(Source::new(text, (3, 14)).is_none());
        let found = unparsed_ignores(text);
        assert_eq!(lines_and_rules(&found), [(2, None), (3, Some(vec!["a"]))]);
        assert_eq!(&text[found[0].span.0..found[0].span.1], "# sqlty: ignore");
        assert!(unparsed_ignores("x = (\n").is_empty());
    }

    #[test]
    fn sql_comments_suppress_on_their_line() {
        let text =
            "-- name: a\nSELECT 1; -- sqlty: ignore[x, y ]\r\n--sqlty:ignore\r-- sqlty: ignored\n";
        let found = sql_ignores(text);
        assert_eq!(
            lines_and_rules(&found),
            [(2, Some(vec!["x", "y"])), (3, None)]
        );
        assert_eq!(&text[found[1].span.0..found[1].span.1], "--sqlty:ignore");
        assert!(sql_ignores("SELECT 1").is_empty());

        let diagnostic = |rule, line| Diagnostic {
            rule,
            message: String::new(),
            severity: crate::rules::Severity::Error,
            line,
            span: 0..1,
            fix: Vec::new(),
        };
        let rules = RuleConfig::default().only(&[Rule::SelectStar, Rule::UnusedIgnore]);
        let kept = apply_sql(&found, vec![diagnostic(Rule::SelectStar, 3)], &rules);
        let messages: Vec<_> = kept.iter().map(|found| found.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "'-- sqlty: ignore[x]' suppresses no diagnostic",
                "'-- sqlty: ignore[y]' suppresses no diagnostic",
            ]
        );
    }

    #[test]
    fn fixes_skip_suppressed_lines_and_the_strings_of_helper_calls() {
        let ignores = [
            Ignore {
                line: 1,
                rules: None,
                span: (0, 1),
            },
            Ignore {
                line: 2,
                rules: Some(vec!["select-star".to_string()]),
                span: (0, 1),
            },
        ];
        let found = suppressed(&ignores, [(1, 3)]);
        assert_eq!(found[&1], None);
        assert_eq!(
            found[&2],
            Some(vec!["select-star".to_string(), "stale-rewrite".to_string()])
        );
        assert_eq!(found[&3], Some(vec!["stale-rewrite".to_string()]));
    }
}
