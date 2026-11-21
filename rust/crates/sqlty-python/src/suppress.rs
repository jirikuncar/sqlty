//! `# sqlty: ignore` comments, and the diagnostics they suppress; `-- sqlty: ignore` in
//! `.sql` files.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use ruff_python_ast::token::TokenKind;
use ruff_python_parser::{lexer, Mode};

use crate::checker::Diagnostic;
use crate::rewriter::LineIndex;
use crate::rows::Reported;
use crate::rules::{Rule, RuleConfig};
use crate::scopes::Lines;
use crate::types::generic::is_word;
use crate::types::symbols::Source;

/// A `# sqlty: ignore` or `# sqlty: ignore[rule, ...]` comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ignore {
    /// The 1-based line it is on.
    pub line: usize,
    /// The first line whose diagnostics it suppresses: `line`, or for a `--` comment alone on
    /// its line the next one that is not a comment.
    pub covers: usize,
    /// The last line it suppresses: `covers`, or the last line of the statement that starts
    /// on `covers` when the comment is alone on its line above it.
    pub through: usize,
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
            let line = source.line(range.start());
            Some(Ignore {
                line,
                covers: line,
                through: line,
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
                    let line = lines.line(range.start().to_usize());
                    found.push(Ignore {
                        line,
                        covers: line,
                        through: line,
                        rules,
                        span: (range.start().to_usize(), range.end().to_usize()),
                    });
                }
            }
            _ => {}
        }
    }
}

/// The `-- sqlty: ignore` comments of a `.sql` file, the first of each line. One alone on its
/// line covers the next line that is not a comment (`--`, or wholly `/* ... */`), through the
/// last line of the one of `statements` (byte ranges) that starts there; a blank line or the
/// end of the file first leaves it covering its own line, where it suppresses nothing. Lines
/// end at `\n`, as the diagnostics of queries and migrations count them, so a lone `\r` ends
/// none. Strings and dollar quotes are not lexed: a line in one that spells the comment counts.
pub fn sql_ignores(text: &str, statements: &[Range<usize>]) -> Vec<Ignore> {
    #[expect(clippy::expect_used, reason = "the pattern is written here")]
    static IGNORE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
        regex::bytes::Regex::new(r"(?-u)--\s*sqlty:\s*ignore\b(?:\[([^\]]*)\])?")
            .expect("a valid pattern")
    });
    if !text.contains("sqlty:") {
        return Vec::new();
    }
    let mut found: Vec<Ignore> = Vec::new();
    let mut waiting: Vec<usize> = Vec::new();
    let mut last_lines = None;
    let mut start = 0;
    for (row, line) in text
        .as_bytes()
        .split_inclusive(|&byte| byte == b'\n')
        .zip(1..)
    {
        let row_start = start;
        start += row.len();
        if let Some(matched) = IGNORE.captures(row) {
            let whole = matched.get_match();
            let alone = row[..whole.start()].iter().all(u8::is_ascii_whitespace);
            found.push(Ignore {
                line,
                covers: line,
                through: line,
                rules: matched.get(1).map(|names| {
                    String::from_utf8_lossy(names.as_bytes())
                        .split(',')
                        .map(|name| name.trim_matches(blank))
                        .filter(|name| !name.is_empty())
                        .map(str::to_string)
                        .collect()
                }),
                span: (row_start + whole.start(), row_start + whole.end()),
            });
            if alone {
                waiting.push(found.len() - 1);
                continue;
            }
        }
        let row = row.trim_ascii();
        if row.is_empty() {
            waiting.clear();
        } else if !is_comment(row) && !waiting.is_empty() {
            let through = last_lines
                .get_or_insert_with(|| statement_ends(text, statements))
                .get(&line)
                .map_or(line, |&last| last.max(line));
            for index in waiting.drain(..) {
                found[index].covers = line;
                found[index].through = through;
            }
        }
    }
    found
}

/// Whether a trimmed line is wholly a comment: `-- ...` or one `/* ... */`.
fn is_comment(row: &[u8]) -> bool {
    row.starts_with(b"--")
        || row.strip_prefix(b"/*").is_some_and(|rest| {
            rest.windows(2).position(|pair| pair == b"*/") == Some(rest.len().wrapping_sub(2))
        })
}

/// The last line of the statements that start on each line, by their first line; built
/// once per file, on the first comment that covers a line below it.
fn statement_ends(text: &str, statements: &[Range<usize>]) -> HashMap<usize, usize> {
    let lines = LineIndex::new(text);
    let mut last: HashMap<usize, usize> = HashMap::new();
    for statement in statements {
        let first = lines.line_at(statement.start);
        let end = lines.line_at(statement.end.saturating_sub(1).max(statement.start));
        let entry = last.entry(first).or_insert(end);
        *entry = (*entry).max(end);
    }
    last
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

/// `apply_suppressions`: `diagnostics` without those a comment covering their line suppresses,
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
    // The comments of each range of lines, and the ranges on each line: comments above one
    // statement share a range, so each line is listed once per range that holds it.
    let mut ranges: HashMap<(usize, usize), Vec<&Ignore>> = HashMap::new();
    for ignore in ignores {
        let range = (ignore.covers, ignore.through);
        ranges.entry(range).or_default().push(ignore);
    }
    let mut covering: HashMap<usize, Vec<&[&Ignore]>> = HashMap::new();
    for (&(first, last), group) in &ranges {
        for line in first..=last {
            covering.entry(line).or_default().push(group);
        }
    }
    let mut kept = Vec::new();
    let mut used: HashSet<(usize, Option<&str>)> = HashSet::new();
    for reported in diagnostics {
        let rule = reported.diagnostic.rule.name();
        let first_naming = |group: &&[&Ignore]| {
            group.iter().find_map(|ignore| match &ignore.rules {
                None => Some((ignore.line, None)),
                Some(names) => names
                    .iter()
                    .any(|name| name == rule)
                    .then_some((ignore.line, Some(rule))),
            })
        };
        // The first comment of the file that covers the line and names the rule.
        let found = covering
            .get(&reported.diagnostic.line)
            .into_iter()
            .flatten()
            .filter_map(first_naming)
            .min_by_key(|&(line, _)| line);
        match found {
            Some(key) => {
                used.insert(key);
            }
            None => kept.push(reported),
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
            "-- name: a\nSELECT 1; -- sqlty: ignore[x, y ]\r\n--sqlty:ignore\r-- sqlty: ignored\nSELECT 2;\n";
        let found = sql_ignores(text, &[]);
        assert_eq!(
            lines_and_rules(&found),
            [(2, Some(vec!["x", "y"])), (3, None)]
        );
        assert_eq!(&text[found[1].span.0..found[1].span.1], "--sqlty:ignore");
        assert!(sql_ignores("SELECT 1", &[]).is_empty());

        let diagnostic = |rule, line| Diagnostic {
            rule,
            message: String::new(),
            severity: crate::rules::Severity::Error,
            line,
            span: 0..1,
            fix: Vec::new(),
        };
        let rules = RuleConfig::default().only(&[Rule::SelectStar, Rule::UnusedIgnore]);
        let kept = apply_sql(&found, vec![diagnostic(Rule::SelectStar, 4)], &rules);
        let messages: Vec<_> = kept.iter().map(|found| found.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "'-- sqlty: ignore[x]' suppresses no diagnostic",
                "'-- sqlty: ignore[y]' suppresses no diagnostic",
            ]
        );
    }

    fn covers(text: &str) -> Vec<(usize, usize)> {
        sql_ignores(text, &[])
            .iter()
            .map(|ignore| (ignore.line, ignore.covers))
            .collect()
    }

    #[test]
    fn a_sql_comment_alone_on_its_line_covers_the_next_statement() {
        assert_eq!(covers("-- sqlty: ignore\nSELECT 1;\n"), [(1, 2)]);
        assert_eq!(
            covers("-- sqlty: ignore[a]\n-- sqlty: ignore[b]\nSELECT 1;\n"),
            [(1, 3), (2, 3)]
        );
        assert_eq!(
            covers("-- sqlty: ignore\n-- name: q\n-- error: x\nSELECT 1;"),
            [(1, 4)]
        );
        assert_eq!(covers("-- sqlty: ignore\n\nSELECT 1;\n"), [(1, 1)]);
        assert_eq!(covers("-- sqlty: ignore\n  \t\nSELECT 1;\n"), [(1, 1)]);
        assert_eq!(covers("SELECT 1;\n-- sqlty: ignore\n"), [(2, 2)]);
        assert_eq!(covers("SELECT 1;\n-- sqlty: ignore\n-- note"), [(2, 2)]);
        assert_eq!(covers("-- sqlty: ignore\r\nSELECT 1;\r\n"), [(1, 2)]);
        assert_eq!(covers("\t-- sqlty: ignore\n\tSELECT 1;\n"), [(1, 2)]);
        assert_eq!(
            covers("SELECT 'é\u{1F600}'; -- sqlty: ignore\nSELECT 1;"),
            [(1, 1)]
        );
        assert_eq!(covers("  -- sqlty: ignore\r\n\tSELECT 1;"), [(1, 2)]);
        assert_eq!(covers("SELECT 1; -- sqlty: ignore\nSELECT 2;"), [(1, 1)]);
        assert_eq!(
            covers("-- sqlty: ignore[a]\nSELECT 1; -- sqlty: ignore[b]\n"),
            [(1, 2), (2, 2)]
        );
        assert_eq!(
            covers("-- sqlty: ignore\nSELECT 1;\n-- sqlty: ignore\n\nSELECT 2;"),
            [(1, 2), (3, 3)]
        );
        // Diagnostics, the CLI and the language server end lines at `\n` only.
        assert_eq!(covers("-- sqlty: ignore\rSELECT 1;\nSELECT 2;"), [(1, 2)]);
        assert_eq!(covers("SELECT 1;\r-- sqlty: ignore\nSELECT 2;"), [(1, 1)]);
        assert_eq!(
            covers("SELECT 1;\r\r-- sqlty: ignore[a]\nSELECT 2;"),
            [(1, 1)]
        );
        let many = "-- sqlty: ignore\n".repeat(1000) + "SELECT 1;";
        assert!(covers(&many).iter().all(|&(_, covered)| covered == 1001));
    }

    fn ranges(text: &str) -> Vec<(usize, usize, usize)> {
        let statements = sqlty_sql::analyzer::statement_ranges(text, "postgres");
        sql_ignores(text, &statements)
            .iter()
            .map(|ignore| (ignore.line, ignore.covers, ignore.through))
            .collect()
    }

    #[test]
    fn a_sql_comment_alone_on_its_line_covers_the_statement_below_it() {
        assert_eq!(
            ranges("-- sqlty: ignore\nSELECT id\nFROM nope\n;\nSELECT 2;"),
            [(1, 2, 3)]
        );
        assert_eq!(
            ranges("-- sqlty: ignore\n/* why */\n  /**/\nSELECT id\nFROM nope;"),
            [(1, 4, 5)]
        );
        // Not wholly a comment: the statement starts on that line.
        assert_eq!(ranges("-- sqlty: ignore\n/* a */ SELECT\n1;"), [(1, 2, 3)]);
        assert_eq!(
            ranges("-- sqlty: ignore\n/* a */ */\nSELECT 1;"),
            [(1, 2, 3)]
        );
        assert_eq!(ranges("-- sqlty: ignore\n/*/\nSELECT 1;"), [(1, 2, 2)]);
        // Two statements start on the line: the cover runs to the end of the longer.
        assert_eq!(
            ranges("-- sqlty: ignore\nSELECT 1; SELECT\n2;\nSELECT 3;"),
            [(1, 2, 3)]
        );
        // Inside a statement, or on a statement's line, a comment covers one line.
        assert_eq!(
            ranges("SELECT a,\n-- sqlty: ignore\nb\nFROM t;"),
            [(2, 3, 3)]
        );
        assert_eq!(ranges("SELECT a -- sqlty: ignore\nFROM t;"), [(1, 1, 1)]);
        let text = "-- sqlty: ignore[unknown-table]\nSELECT id\nFROM nope;\n";
        let diagnostic = Diagnostic {
            rule: Rule::UnknownTable,
            message: String::new(),
            severity: crate::rules::Severity::Error,
            line: 3,
            span: 22..26,
            fix: Vec::new(),
        };
        let statements = sqlty_sql::analyzer::statement_ranges(text, "postgres");
        let rules = RuleConfig::default().only(&[Rule::UnknownTable, Rule::UnusedIgnore]);
        let kept = apply_sql(&sql_ignores(text, &statements), vec![diagnostic], &rules);
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn a_second_comment_for_one_statement_is_unused() {
        let text =
            "-- sqlty: ignore[select-star]\nSELECT * FROM t; -- sqlty: ignore[select-star]\n";
        let diagnostic = Diagnostic {
            rule: Rule::SelectStar,
            message: String::new(),
            severity: crate::rules::Severity::Error,
            line: 2,
            span: 0..1,
            fix: Vec::new(),
        };
        let rules = RuleConfig::default().only(&[Rule::SelectStar, Rule::UnusedIgnore]);
        let kept = apply_sql(&sql_ignores(text, &[]), vec![diagnostic], &rules);
        let [unused] = kept.as_slice() else {
            panic!("{kept:?}");
        };
        assert_eq!((unused.rule, unused.line), (Rule::UnusedIgnore, 2));
    }

    #[test]
    fn fixes_skip_suppressed_lines_and_the_strings_of_helper_calls() {
        let ignores = [
            Ignore {
                line: 1,
                covers: 1,
                through: 1,
                rules: None,
                span: (0, 1),
            },
            Ignore {
                line: 2,
                covers: 2,
                through: 2,
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
