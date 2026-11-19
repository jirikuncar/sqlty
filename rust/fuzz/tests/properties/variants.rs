//! Query variants: functions of three booleans built from `if`/`elif`/`else`, `match`,
//! conditional expressions, `+=`, `return` and `raise` over string literals, against what
//! sqlty reads. A model runs each function for all eight inputs to collect the queries it
//! passes to `run`; every one must be a variant sqlty reads (soundness), and when each
//! condition is a plain parameter tested the same way everywhere, the variants are exactly
//! those queries (precision).

use std::collections::BTreeSet;

use proptest::prelude::*;
use ruff_python_ast::Expr;
use ruff_text_size::Ranged;
use sqlty_python::scopes::Scan;
use sqlty_python::types::symbols::Source;
use sqlty_python::variants::{query_variants, Found};

use crate::draw::{check, Draw};
use crate::engine::VERSION;

const FLAGS: [&str; 3] = ["a", "b", "c"];
const WORDS: [&str; 4] = ["x", "y", "z", "w"];
const LEAVES: usize = 12;

#[derive(Debug, Clone, Copy)]
struct Cond {
    flag: usize,
    negated: bool,
}

#[derive(Debug)]
enum Stmt {
    Append(&'static str),
    SetQ(&'static str),
    SetX(&'static str),
    AppendX,
    AppendEither(Cond, &'static str, &'static str),
    Return,
    Raise,
    If(Cond, Vec<Stmt>, Option<Vec<Stmt>>),
    Elif(Cond, Cond, Vec<Stmt>, Vec<Stmt>, Vec<Stmt>),
    Match(usize, Vec<Stmt>, Vec<Stmt>),
}

fn cond(d: &mut Draw<'_>, negations: bool) -> Cond {
    Cond {
        flag: d.int(0, 2),
        negated: negations && d.bool(),
    }
}

/// A block of nested `if`s, of up to `LEAVES` statements and blocks.
fn block(d: &mut Draw<'_>, negations: bool, leaves: &mut usize) -> Vec<Stmt> {
    *leaves = leaves.saturating_sub(1);
    if *leaves == 0 || d.int(0, 2) == 0 {
        return vec![match d.int(0, 6) {
            0 => Stmt::Append(d.pick(&WORDS)),
            1 => Stmt::SetQ(d.pick(&WORDS)),
            2 => Stmt::SetX(d.pick(&WORDS)),
            3 => Stmt::AppendX,
            4 => Stmt::AppendEither(cond(d, negations), d.pick(&WORDS), d.pick(&WORDS)),
            5 => Stmt::Return,
            _ => Stmt::Raise,
        }];
    }
    match d.int(0, 3) {
        0 => vec![Stmt::If(
            cond(d, negations),
            block(d, negations, leaves),
            Some(block(d, negations, leaves)),
        )],
        1 => vec![Stmt::If(
            cond(d, negations),
            block(d, negations, leaves),
            None,
        )],
        2 => {
            let (first, second) = (cond(d, negations), cond(d, negations));
            vec![Stmt::Elif(
                first,
                second,
                block(d, negations, leaves),
                block(d, negations, leaves),
                block(d, negations, leaves),
            )]
        }
        _ => d
            .list(1, 3, |d| block(d, negations, leaves))
            .into_iter()
            .flatten()
            .collect(),
    }
}

fn statements(d: &mut Draw<'_>, negations: bool) -> Vec<Stmt> {
    block(d, negations, &mut LEAVES.clone())
}

fn matches(d: &mut Draw<'_>) -> Vec<Stmt> {
    let flag = d.int(0, 2);
    vec![Stmt::Match(
        flag,
        statements(d, false),
        statements(d, false),
    )]
}

fn spell(cond: Cond) -> String {
    let flag = FLAGS[cond.flag];
    if cond.negated {
        format!("not {flag}")
    } else {
        flag.to_string()
    }
}

fn write(stmts: &[Stmt], depth: usize, lines: &mut Vec<String>) {
    let pad = "    ".repeat(depth);
    if stmts.is_empty() {
        lines.push(format!("{pad}pass"));
    }
    for stmt in stmts {
        match stmt {
            Stmt::Append(w) => lines.push(format!("{pad}q += \" {w}\"")),
            Stmt::SetQ(w) => lines.push(format!("{pad}q = \"{w}\"")),
            Stmt::SetX(w) => lines.push(format!("{pad}x = \"{w}\"")),
            Stmt::AppendX => lines.push(format!("{pad}q += x")),
            Stmt::AppendEither(c, yes, no) => lines.push(format!(
                "{pad}q += \" {yes}\" if {} else \" {no}\"",
                spell(*c)
            )),
            Stmt::Return => lines.push(format!("{pad}return")),
            Stmt::Raise => lines.push(format!("{pad}raise ValueError")),
            Stmt::If(c, then, otherwise) => {
                lines.push(format!("{pad}if {}:", spell(*c)));
                write(then, depth + 1, lines);
                if let Some(otherwise) = otherwise {
                    lines.push(format!("{pad}else:"));
                    write(otherwise, depth + 1, lines);
                }
            }
            Stmt::Elif(first, second, a, b, c) => {
                lines.push(format!("{pad}if {}:", spell(*first)));
                write(a, depth + 1, lines);
                lines.push(format!("{pad}elif {}:", spell(*second)));
                write(b, depth + 1, lines);
                lines.push(format!("{pad}else:"));
                write(c, depth + 1, lines);
            }
            Stmt::Match(flag, yes, no) => {
                lines.push(format!("{pad}match {}:", FLAGS[*flag]));
                lines.push(format!("{pad}    case True:"));
                write(yes, depth + 2, lines);
                lines.push(format!("{pad}    case _:"));
                write(no, depth + 2, lines);
            }
        }
    }
}

fn function(body: &[Stmt]) -> String {
    let mut lines = vec![
        "def f(a: bool, b: bool, c: bool) -> None:".to_string(),
        "    q = \"SELECT\"".to_string(),
        "    x = \"v\"".to_string(),
    ];
    write(body, 1, &mut lines);
    lines.push("    run(q)".to_string());
    lines.join("\n") + "\n"
}

/// How a block ends: falling through, `return` or `raise`.
#[derive(PartialEq)]
enum Flow {
    Next,
    Stop,
}

struct State {
    q: String,
    x: String,
    flags: [bool; 3],
}

impl State {
    fn holds(&self, cond: Cond) -> bool {
        self.flags[cond.flag] != cond.negated
    }

    fn run(&mut self, stmts: &[Stmt]) -> Flow {
        for stmt in stmts {
            let flow = match stmt {
                Stmt::Append(w) => {
                    self.q += &format!(" {w}");
                    Flow::Next
                }
                Stmt::SetQ(w) => {
                    self.q = (*w).to_string();
                    Flow::Next
                }
                Stmt::SetX(w) => {
                    self.x = (*w).to_string();
                    Flow::Next
                }
                Stmt::AppendX => {
                    self.q += &self.x.clone();
                    Flow::Next
                }
                Stmt::AppendEither(c, yes, no) => {
                    self.q += &format!(" {}", if self.holds(*c) { yes } else { no });
                    Flow::Next
                }
                Stmt::Return | Stmt::Raise => Flow::Stop,
                Stmt::If(c, then, otherwise) => match (self.holds(*c), otherwise) {
                    (true, _) => self.run(then),
                    (false, Some(otherwise)) => self.run(otherwise),
                    (false, None) => Flow::Next,
                },
                Stmt::Elif(first, second, a, b, c) => {
                    match (self.holds(*first), self.holds(*second)) {
                        (true, _) => self.run(a),
                        (false, true) => self.run(b),
                        (false, false) => self.run(c),
                    }
                }
                Stmt::Match(flag, yes, no) => {
                    if self.flags[*flag] {
                        self.run(yes)
                    } else {
                        self.run(no)
                    }
                }
            };
            if flow == Flow::Stop {
                return Flow::Stop;
            }
        }
        Flow::Next
    }
}

/// The queries `f` passes to `run` over all eight inputs.
fn concrete(body: &[Stmt]) -> BTreeSet<String> {
    (0..8)
        .filter_map(|bits: usize| {
            let flags = [bits & 1 != 0, bits & 2 != 0, bits & 4 != 0];
            let mut state = State {
                q: "SELECT".into(),
                x: "v".into(),
                flags,
            };
            (state.run(body) == Flow::Next).then_some(state.q)
        })
        .collect()
}

/// The variants sqlty reads of the query passed to `run`, or None and why it is unchecked.
fn read(source: &str) -> (Option<BTreeSet<String>>, Option<String>) {
    let Some(source) = Source::new(source, VERSION) else {
        return (None, None);
    };
    let scan = Scan::new(source.body(), |call| {
        call.func
            .as_name_expr()
            .is_some_and(|name| name.id.as_str() == "run")
    });
    let Some((call, stack)) = scan.calls.first() else {
        return (None, None);
    };
    let query: &Expr = &call.arguments.args[0];
    let stack: Vec<_> = stack.iter().map(|scope| scan.scopes[*scope]).collect();
    match query_variants(&source, &stack, call.range(), query, 1_000_000, None) {
        None => (None, None),
        Some(Found::Unchecked { reason, .. }) => (None, reason),
        Some(Found::Variants(variants)) => {
            (Some(variants.into_iter().map(|v| v.query).collect()), None)
        }
    }
}

fn compare(body: &[Stmt], precise: bool) -> Result<(), TestCaseError> {
    let source = function(body);
    let concrete = concrete(body);
    let (read, reason) = read(&source);
    let Some(read) = read else {
        // `run` is never reached.
        prop_assert!(
            concrete.is_empty() && reason.is_none(),
            "read as unchecked ({:?}):\n{}",
            reason,
            source
        );
        return Ok(());
    };
    let missed: Vec<_> = concrete.difference(&read).collect();
    prop_assert!(missed.is_empty(), "missed {:?}:\n{}", missed, source);
    if precise {
        let infeasible: Vec<_> = read.difference(&concrete).collect();
        prop_assert!(
            infeasible.is_empty(),
            "infeasible {:?}:\n{}",
            infeasible,
            source
        );
    }
    Ok(())
}

#[test]
fn variants_are_exact_with_plain_conditions() {
    check(|d| statements(d, false), |body| compare(&body, true));
}

#[test]
fn variants_cover_every_query() {
    let case = |d: &mut Draw<'_>| -> Vec<Stmt> {
        d.list(1, 3, |d| {
            if d.bool() {
                statements(d, true)
            } else {
                matches(d)
            }
        })
        .into_iter()
        .flatten()
        .collect()
    };
    check(case, |body| compare(&body, false));
}
