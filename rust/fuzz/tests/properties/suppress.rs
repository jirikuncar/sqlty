//! `-- sqlty: ignore` in `.sql` files: random lines of comments, blank lines, statements,
//! strings and dollar quotes spelling the comment, unicode, with `\n`, `\r\n` and `\r` endings.
//! A model of the documented rule says which line each comment covers and what `apply_sql`
//! keeps: a comment alone on its line covers the next line that is not a `--` comment, unless
//! a blank line comes first; any other covers its own line. Lines end at `\n`, as the
//! diagnostics, the CLI and the language server count them: a lone `\r` joins two drawn lines.

use proptest::prelude::*;
use sqlty_python::checker::Diagnostic;
use sqlty_python::rules::{Rule, RuleConfig, Severity};
use sqlty_python::suppress::{apply_sql, sql_ignores};

use crate::draw::{check, Draw};

/// A line, and the first ignore comment on it: its rules (`None`: every rule) and whether
/// only ASCII whitespace comes before it.
type Ignore = Option<(Option<&'static [&'static str]>, bool)>;

#[rustfmt::skip]
const LINES: [(&str, Ignore); 20] = [
    ("", None),
    ("  \t", None),
    ("-- note", None),
    ("-- sqlty: ignored", None),
    ("-- sqlty: ignore", Some((None, true))),
    ("-- sqlty: ignore[select-star]", Some((Some(&["select-star"]), true))),
    ("\t--sqlty:ignore[unknown-column, select-star ]", Some((Some(&["unknown-column", "select-star"]), true))),
    ("-- sqlty: ignore[nope]", Some((Some(&["nope"]), true))),
    ("SELECT * FROM t;", None),
    ("SELECT 1; -- sqlty: ignore[select-star]", Some((Some(&["select-star"]), false))),
    ("SELECT '-- sqlty: ignore';", Some((None, false))),
    ("$$", None),
    ("SELECT 'é😀'; -- sqlty: ignore", Some((None, false))),
    ("\u{a0}-- sqlty: ignore", Some((None, false))),
    ("/* -- sqlty: ignore[unknown-column] */", Some((Some(&["unknown-column"]), false))),
    ("-- name: q", None),
    ("-- sqlty: ignore[select-star] -- sqlty: ignore", Some((Some(&["select-star"]), true))),
    ("  -- sqlty: ignore[]", Some((Some(&[]), true))),
    ("-- sqlty: ignore[select-star", Some((None, true))),
    ("\x0b-- sqlty: ignore", Some((None, false))),
];
const ENDINGS: [&str; 3] = ["\n", "\r\n", "\r"];
const RULES: [Rule; 2] = [Rule::SelectStar, Rule::UnknownColumn];

#[derive(Debug)]
struct Case {
    /// Each line's index in `LINES` and its ending (`""` for the last line without one).
    lines: Vec<(usize, &'static str)>,
    /// A diagnostic's line and rule.
    diagnostics: Vec<(usize, Rule)>,
}

fn case(d: &mut Draw<'_>) -> Case {
    let count = d.int(0, 12);
    let mut lines: Vec<(usize, &'static str)> = Vec::new();
    for at in 0..count {
        let line = d.int(0, LINES.len() - 1);
        let mut ending = d.pick(&ENDINGS);
        // An unclosed `[` would take a `]` after the `\r` into its rules.
        if at + 1 == count && d.bool() {
            ending = "";
        } else if ending == "\r" && LINES[line].0.ends_with("select-star") {
            ending = "\n";
        }
        lines.push((line, ending));
    }
    let diagnostics = d.list(0, 6, |d| (d.int(1, count + 1), d.pick(&RULES)));
    Case { lines, diagnostics }
}

/// What the line counts as for a comment waiting above it.
#[derive(PartialEq)]
enum Class {
    Blank,
    Comment,
    Other,
}

fn class(text: &str) -> Class {
    let text = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    if text.is_empty() {
        Class::Blank
    } else if text.starts_with("--") {
        Class::Comment
    } else {
        Class::Other
    }
}

/// The drawn lines of each line the file has, joined by `\r`.
fn joined(lines: &[(usize, &str)]) -> Vec<Vec<usize>> {
    let mut found = vec![Vec::new()];
    for &(index, ending) in lines {
        found.last_mut().expect("a line").push(index);
        if ending != "\r" {
            found.push(Vec::new());
        }
    }
    found.retain(|line| !line.is_empty());
    found
}

/// `(line, covers, rules)` of each ignore comment, by the model.
fn model(lines: &[(usize, &str)]) -> Vec<(usize, usize, Option<&'static [&'static str]>)> {
    let lines = joined(lines);
    let text = |line: &Vec<usize>| -> String {
        let parts: Vec<_> = line.iter().map(|&index| LINES[index].0).collect();
        parts.join("\r")
    };
    let mut found = Vec::new();
    for (at, parts) in lines.iter().enumerate() {
        // The first ignore of the line, alone when only ASCII whitespace comes before it.
        let Some((part, (rules, alone))) = parts
            .iter()
            .enumerate()
            .find_map(|(part, &index)| Some((part, LINES[index].1?)))
        else {
            continue;
        };
        let before = parts[..part].iter().map(|&index| LINES[index].0);
        let alone = alone && before.collect::<String>().trim_ascii().is_empty();
        let line = at + 1;
        let mut covers = line;
        if alone {
            for (below, parts) in lines.iter().enumerate().skip(at + 1) {
                match class(&text(parts)) {
                    Class::Blank => break,
                    Class::Comment => {}
                    Class::Other => {
                        covers = below + 1;
                        break;
                    }
                }
            }
        }
        found.push((line, covers, rules));
    }
    found
}

fn diagnostic(rule: Rule, line: usize, message: String) -> Diagnostic {
    Diagnostic {
        rule,
        message,
        severity: Severity::Error,
        line,
        span: 0..0,
        fix: Vec::new(),
    }
}

#[test]
fn sql_ignores_cover_the_line_the_model_says() {
    let rules = RuleConfig::from_overrides(Some(
        [("unused-ignore".to_string(), "error".to_string())].into(),
    ))
    .expect("known rules");
    check(case, |case| {
        let text: String = case
            .lines
            .iter()
            .flat_map(|&(index, ending)| [LINES[index].0, ending])
            .collect();
        let expected = model(&case.lines);
        let found = sql_ignores(&text);
        let lines: Vec<_> = found
            .iter()
            .map(|ignore| (ignore.line, ignore.covers))
            .collect();
        let want: Vec<_> = expected
            .iter()
            .map(|&(line, covers, _)| (line, covers))
            .collect();
        prop_assert_eq!(&lines, &want, "{:?}", text);
        prop_assert!(found.iter().all(|ignore| ignore.covers >= ignore.line));

        // Each diagnostic is dropped by the first comment covering its line that names its
        // rule; every rule of a comment that dropped nothing is an `unused-ignore` on its line.
        let mut used = std::collections::HashSet::new();
        let mut want = Vec::new();
        let mut diagnostics = Vec::new();
        for (k, &(line, rule)) in case.diagnostics.iter().enumerate() {
            let message = format!("d{k}");
            diagnostics.push(diagnostic(rule, line, message.clone()));
            let by = expected.iter().find_map(|&(at, covers, names)| {
                let named = match names {
                    None => true,
                    Some(names) => names.contains(&rule.name()),
                };
                (covers == line && named).then_some((at, names.map(|_| rule.name())))
            });
            match by {
                Some(key) => {
                    used.insert(key);
                }
                None => want.push((rule.name(), line, message)),
            }
        }
        let comment_lines: Vec<usize> = expected.iter().map(|&(line, _, _)| line).collect();
        for &(line, _, names) in &expected {
            let unused: Vec<Option<&str>> = match names {
                None => vec![None],
                Some(names) => names.iter().map(|name| Some(*name)).collect(),
            };
            for name in unused {
                if !used.contains(&(line, name)) {
                    let written = name.map_or("-- sqlty: ignore".to_string(), |name| {
                        format!("-- sqlty: ignore[{name}]")
                    });
                    let message = format!("'{written}' suppresses no diagnostic");
                    want.push(("unused-ignore", line, message));
                }
            }
        }
        let kept = apply_sql(&found, diagnostics, &rules);
        for unused in kept.iter().filter(|kept| kept.rule == Rule::UnusedIgnore) {
            prop_assert!(comment_lines.contains(&unused.line), "{unused:?}");
        }
        let mut kept: Vec<_> = kept
            .into_iter()
            .map(|kept| (kept.rule.name(), kept.line, kept.message))
            .collect();
        kept.sort();
        want.sort();
        prop_assert_eq!(kept, want, "{:?}", text);
        Ok(())
    });
}
