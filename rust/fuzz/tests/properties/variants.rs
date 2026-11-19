//! Query variants: functions of three booleans built from `if`/`elif`/`else`, `match`,
//! conditional expressions, `+=`, `return` and `raise` over string literals, against what
//! sqlty reads. A model runs each function for all eight inputs to collect the queries it
//! passes to `run`; every one must be a variant sqlty reads (soundness), and when each
//! condition is a plain parameter tested the same way everywhere, the variants are exactly
//! those queries (precision).

use std::collections::BTreeSet;

use proptest::prelude::*;
use ruff_text_size::Ranged;
use sqlty_python::scopes::Scan;
use sqlty_python::types::symbols::Source;
use sqlty_python::variants::{query_variants, Found};

use crate::draw::{check, Draw};
use crate::engine::VERSION;

const FLAGS: [&str; 3] = ["a", "b", "c"];
const WORDS: [&str; 4] = ["x", "y", "z", "w"];

/// A flag, and whether it is negated.
type Cond = (usize, bool);

#[derive(Debug)]
enum Stmt {
    /// `q += " w"`, `q = "w"` or `x = "w"`.
    Assign(&'static str, &'static str),
    AppendX,
    Either(Cond, &'static str, &'static str),
    /// `return` or `raise`.
    Stop(&'static str),
    /// `if`/`elif`/`else`, or `match` when `match`: the first arm whose condition holds runs.
    Branch {
        r#match: bool,
        arms: Vec<(Option<Cond>, Vec<Stmt>)>,
    },
}

fn cond(d: &mut Draw<'_>, negations: bool) -> Cond {
    (d.int(0, 2), negations && d.bool())
}

/// A block of nested `if`s of up to `leaves` statements and blocks.
fn block(d: &mut Draw<'_>, negations: bool, leaves: &mut usize) -> Vec<Stmt> {
    *leaves = leaves.saturating_sub(1);
    if *leaves == 0 || d.int(0, 2) == 0 {
        return vec![match d.int(0, 6) {
            0 => Stmt::Assign("q +=", d.pick(&WORDS)),
            1 => Stmt::Assign("q =", d.pick(&WORDS)),
            2 => Stmt::Assign("x =", d.pick(&WORDS)),
            3 => Stmt::AppendX,
            4 => Stmt::Either(cond(d, negations), d.pick(&WORDS), d.pick(&WORDS)),
            5 => Stmt::Stop("return"),
            _ => Stmt::Stop("raise ValueError"),
        }];
    }
    let mut arm = |d: &mut Draw<'_>, test: bool| {
        (
            test.then(|| cond(d, negations)),
            block(d, negations, leaves),
        )
    };
    let arms = match d.int(0, 3) {
        0 => vec![arm(d, true), arm(d, false)],
        1 => vec![arm(d, true)],
        2 => vec![arm(d, true), arm(d, true), arm(d, false)],
        _ => {
            return d
                .list(1, 3, |d| block(d, negations, leaves))
                .into_iter()
                .flatten()
                .collect()
        }
    };
    vec![Stmt::Branch {
        r#match: false,
        arms,
    }]
}

fn statements(d: &mut Draw<'_>, negations: bool) -> Vec<Stmt> {
    block(d, negations, &mut 12)
}

fn matches(d: &mut Draw<'_>) -> Stmt {
    let flag = d.int(0, 2);
    Stmt::Branch {
        r#match: true,
        arms: vec![
            (Some((flag, false)), statements(d, false)),
            (None, statements(d, false)),
        ],
    }
}

fn spell((flag, negated): Cond) -> String {
    format!("{}{}", if negated { "not " } else { "" }, FLAGS[flag])
}

fn write(stmts: &[Stmt], pad: &str, lines: &mut Vec<String>) {
    if stmts.is_empty() {
        lines.push(format!("{pad}pass"));
    }
    for stmt in stmts {
        match stmt {
            Stmt::Assign("q +=", w) => lines.push(format!("{pad}q += \" {w}\"")),
            Stmt::Assign(target, w) => lines.push(format!("{pad}{target} \"{w}\"")),
            Stmt::AppendX => lines.push(format!("{pad}q += x")),
            Stmt::Either(c, yes, no) => lines.push(format!(
                "{pad}q += \" {yes}\" if {} else \" {no}\"",
                spell(*c)
            )),
            Stmt::Stop(stop) => lines.push(format!("{pad}{stop}")),
            Stmt::Branch {
                r#match: true,
                arms,
            } => {
                lines.push(format!(
                    "{pad}match {}:",
                    FLAGS[arms[0].0.map_or(0, |c| c.0)]
                ));
                for (test, body) in arms {
                    lines.push(format!(
                        "{pad}    case {}:",
                        if test.is_some() { "True" } else { "_" }
                    ));
                    write(body, &format!("{pad}        "), lines);
                }
            }
            Stmt::Branch { arms, .. } => {
                for (i, (test, body)) in arms.iter().enumerate() {
                    lines.push(match (i, test) {
                        (0, Some(c)) => format!("{pad}if {}:", spell(*c)),
                        (_, Some(c)) => format!("{pad}elif {}:", spell(*c)),
                        (_, None) => format!("{pad}else:"),
                    });
                    write(body, &format!("{pad}    "), lines);
                }
            }
        }
    }
}

fn function(body: &[Stmt]) -> String {
    let mut lines = vec![
        "def f(a: bool, b: bool, c: bool) -> None:".into(),
        "    q = \"SELECT\"".into(),
        "    x = \"v\"".into(),
    ];
    write(body, "    ", &mut lines);
    lines.push("    run(q)".into());
    lines.join("\n") + "\n"
}

/// Runs `stmts` on `(q, x)` with `flags`; false once a `return` or `raise` ends the call.
fn run(stmts: &[Stmt], flags: [bool; 3], state: &mut (String, String)) -> bool {
    let holds = |(flag, negated): Cond| flags[flag] != negated;
    stmts.iter().all(|stmt| match stmt {
        Stmt::Assign("q +=", w) => {
            state.0 += &format!(" {w}");
            true
        }
        Stmt::Assign("q =", w) => {
            state.0 = (*w).into();
            true
        }
        Stmt::Assign(_, w) => {
            state.1 = (*w).into();
            true
        }
        Stmt::AppendX => {
            state.0 += &state.1.clone();
            true
        }
        Stmt::Either(c, yes, no) => {
            state.0 += &format!(" {}", if holds(*c) { yes } else { no });
            true
        }
        Stmt::Stop(_) => false,
        Stmt::Branch { arms, .. } => arms
            .iter()
            .find(|(test, _)| test.is_none_or(holds))
            .is_none_or(|(_, body)| run(body, flags, state)),
    })
}

/// The queries `f` passes to `run` over all eight inputs.
fn concrete(body: &[Stmt]) -> BTreeSet<String> {
    (0..8)
        .filter_map(|bits: usize| {
            let mut state = ("SELECT".into(), "v".into());
            run(
                body,
                [bits & 1 != 0, bits & 2 != 0, bits & 4 != 0],
                &mut state,
            )
            .then_some(state.0)
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
    let stack: Vec<_> = stack.iter().map(|scope| scan.scopes[*scope]).collect();
    match query_variants(
        &source,
        &stack,
        call.range(),
        &call.arguments.args[0],
        1_000_000,
        None,
    ) {
        None => (None, None),
        Some(Found::Unchecked { reason, .. }) => (None, reason),
        Some(Found::Variants(variants)) => {
            (Some(variants.into_iter().map(|v| v.query).collect()), None)
        }
    }
}

fn compare(body: &[Stmt], precise: bool) -> Result<(), TestCaseError> {
    let (source, concrete) = (function(body), concrete(body));
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
    let infeasible: Vec<_> = read.difference(&concrete).collect();
    prop_assert!(
        !precise || infeasible.is_empty(),
        "infeasible {:?}:\n{}",
        infeasible,
        source
    );
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
                vec![matches(d)]
            }
        })
        .into_iter()
        .flatten()
        .collect()
    };
    check(case, |body| compare(&body, false));
}
