//! Which bindings of a name reach a use, and what an `isinstance` test on the way says of
//! it: one structured walk of the statements of a function (or of the module), made for
//! the name and the use asked about.
//!
//! The walk follows the statements as they run: a binding replaces what the name held,
//! the branches of an `if`, `try` or `match` are walked apart and merged, a loop body
//! starts from what the name holds before the loop and from every binding in the loop,
//! and a branch that ends in `return`, `raise`, `continue` or `break` does not reach what
//! follows. The bindings themselves come from [`crate::scopes::Scan`], which records
//! every form that binds a name; this module adds only the order they run in.

use ruff_python_ast::{self as ast, BoolOp, Expr, Stmt, UnaryOp};
use ruff_text_size::Ranged;

use crate::scopes::{Binding, Origin};

/// What a name holds at a use.
pub struct Reach<'a> {
    /// The bindings that can be in effect; none when the use is not reached.
    pub origins: Vec<(Origin<'a>, u32)>,
    /// The class of an `isinstance` test every path to the use passed, when the name was
    /// not bound again since.
    pub narrowed: Option<&'a Expr>,
}

/// What `name` holds at offset `at` of `body`, the statements of the scope whose
/// bindings of the name are `bindings` (see [`crate::scopes::Scan::bindings`]). `None`
/// when `at` is not in `body`: a decorator or a default.
pub fn reaching<'a>(
    body: &'a [Stmt],
    bindings: &[Binding<'a>],
    name: &str,
    at: u32,
) -> Option<Reach<'a>> {
    let (first, last) = (body.first()?, body.last()?);
    if at < first.start().to_u32() || at >= last.end().to_u32() {
        return None;
    }
    let walk = Walk { bindings, name, at };
    // What is bound before the body: the parameters.
    let start = walk.apply(State::default(), 0, first.start().to_u32(), u32::MAX);
    let state = match walk.block(body, start, 0) {
        Out::Found(state) => state,
        Out::Next(_) | Out::Stop => State::default(),
    };
    if state.lost || state.twice {
        return Some(Reach {
            origins: vec![(Origin::Unknown, at)],
            narrowed: None,
        });
    }
    Some(Reach {
        origins: state
            .live
            .iter()
            .map(|&index| (bindings[index].origin, bindings[index].site))
            .collect(),
        narrowed: state.narrowed,
    })
}

/// The outermost `for` or `while` of `body` around offset `at`, as its start and end.
pub fn loop_around(body: &[Stmt], at: u32) -> Option<(u32, u32)> {
    let index = body.partition_point(|stmt| stmt.end().to_u32() <= at);
    let stmt = body.get(index).filter(|stmt| stmt.start().to_u32() <= at)?;
    let inner = |blocks: &[&[Stmt]]| blocks.iter().find_map(|block| loop_around(block, at));
    match stmt {
        Stmt::For(_) | Stmt::While(_) => Some((stmt.start().to_u32(), stmt.end().to_u32())),
        Stmt::If(node) => {
            let clauses = node.elif_else_clauses.iter().map(|clause| &*clause.body);
            let blocks: Vec<&[Stmt]> = std::iter::once(&*node.body).chain(clauses).collect();
            inner(&blocks)
        }
        Stmt::With(node) => inner(&[&node.body]),
        Stmt::Try(node) => {
            let handlers = node.handlers.iter().map(|handler| {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                &*handler.body
            });
            let rest = [&*node.orelse, &*node.finalbody];
            let blocks: Vec<&[Stmt]> = std::iter::once(&*node.body)
                .chain(handlers)
                .chain(rest)
                .collect();
            inner(&blocks)
        }
        Stmt::Match(node) => {
            let blocks: Vec<&[Stmt]> = node.cases.iter().map(|case| &*case.body).collect();
            inner(&blocks)
        }
        _ => None,
    }
}

/// The most bindings followed at once: a name more branches than this bind is unknown.
const MAX_LIVE: usize = 64;

/// The bindings in effect, by index in the order written, and the class the name was
/// tested to be of.
#[derive(Clone, Default)]
struct State<'a> {
    live: Vec<usize>,
    narrowed: Option<&'a Expr>,
    /// The name was tested again after `narrowed`: it is of both classes, which is not
    /// followed.
    twice: bool,
    /// More than [`MAX_LIVE`] bindings reach: what the name holds is not followed.
    lost: bool,
}

impl<'a> State<'a> {
    /// The state where `class` is what a test says of the name. A second test of a name
    /// already narrowed is not followed.
    fn narrow(mut self, class: Option<&'a Expr>) -> Self {
        if class.is_some() {
            self.twice = self.narrowed.is_some();
            self.narrowed = class;
        }
        self
    }

    /// Either state: the bindings of both, and the test both passed.
    fn merge(mut self, other: &State<'a>) -> Self {
        work(self.live.len() + other.live.len());
        let mut live = Vec::with_capacity(self.live.len() + other.live.len());
        let (mut one, mut two) = (self.live.iter().peekable(), other.live.iter().peekable());
        loop {
            let next = match (one.peek(), two.peek()) {
                (Some(&&a), Some(&&b)) if a == b => {
                    two.next();
                    one.next();
                    a
                }
                (Some(&&a), Some(&&b)) if a < b => {
                    one.next();
                    a
                }
                (_, Some(&&b)) => {
                    two.next();
                    b
                }
                (Some(&&a), None) => {
                    one.next();
                    a
                }
                (None, None) => break,
            };
            live.push(next);
        }
        // What only one side tested is not known of the other: the name is then what
        // its bindings give it.
        let same = match (self.narrowed, other.narrowed) {
            (Some(one), Some(two)) => std::ptr::eq(one, two) && self.twice == other.twice,
            _ => false,
        };
        if !same {
            self.narrowed = None;
            self.twice = false;
        }
        self.lost |= other.lost || live.len() > MAX_LIVE;
        live.truncate(MAX_LIVE);
        self.live = live;
        self
    }
}

#[cfg(test)]
thread_local! {
    /// The statements walked and bindings merged by this thread, for the tests of cost.
    static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts `steps` of work in tests; nothing otherwise.
#[cfg_attr(
    not(test),
    expect(clippy::missing_const_for_fn, reason = "it counts in tests")
)]
fn work(steps: usize) {
    #[cfg(test)]
    WORK.with(|work| work.set(work.get() + steps));
    #[cfg(not(test))]
    let _ = steps;
}

/// How a statement or a block ends.
enum Out<'a> {
    /// The use is in it: what the name holds there.
    Found(State<'a>),
    /// It runs on to the next statement.
    Next(State<'a>),
    /// It does not: `return`, `raise`, `continue`, `break`.
    Stop,
}

/// The branches that run on, merged; `Stop` when none does.
fn merged<'a>(outs: Vec<Out<'a>>) -> Out<'a> {
    let mut found: Option<State<'a>> = None;
    for out in outs {
        if let Out::Next(state) = out {
            found = Some(match found {
                Some(known) => known.merge(&state),
                None => state,
            });
        }
    }
    found.map_or(Out::Stop, Out::Next)
}

/// How deep statements nest before the walk gives up on the use.
const MAX_DEPTH: u8 = 64;

struct Walk<'s, 'a> {
    bindings: &'s [Binding<'a>],
    name: &'s str,
    at: u32,
}

impl<'a> Walk<'_, 'a> {
    fn within(&self, start: u32, end: u32) -> bool {
        start <= self.at && self.at < end
    }

    /// The bindings written in `start..end`, by index: they are in the order written.
    fn written(&self, start: u32, end: u32) -> std::ops::Range<usize> {
        let from = self
            .bindings
            .partition_point(|binding| binding.site < start);
        let to = self.bindings.partition_point(|binding| binding.site < end);
        from..to.max(from)
    }

    /// `state` after the bindings written in `start..end` that are in effect by `limit`.
    fn apply(&self, state: State<'a>, start: u32, end: u32, limit: u32) -> State<'a> {
        let last = self
            .written(start, end)
            .rev()
            .find(|&index| self.bindings[index].effect <= limit);
        match last {
            Some(index) => State {
                live: vec![index],
                ..State::default()
            },
            None => state,
        }
    }

    /// `state` or any binding written in `start..end`: what a loop body or an exception
    /// handler starts from.
    fn any(&self, mut state: State<'a>, start: u32, end: u32) -> State<'a> {
        let written = self.written(start, end);
        if written.is_empty() {
            return state;
        }
        let entered = State {
            live: written.take(MAX_LIVE + 1).collect(),
            ..State::default()
        };
        state.narrowed = None;
        state.twice = false;
        state.merge(&entered)
    }

    /// What `test` says of the name when it holds and when it does not; nothing when the
    /// test itself binds the name (`isinstance(x, C) and (x := y)`).
    fn tested(&self, test: &'a Expr) -> (Option<&'a Expr>, Option<&'a Expr>) {
        let written = self.written(test.start().to_u32(), test.end().to_u32());
        match written.is_empty() {
            true => narrowing(test, self.name),
            false => (None, None),
        }
    }

    /// The part of a compound statement before its first block: `state` after it, and
    /// whether the use is there.
    fn header(&self, state: State<'a>, start: u32, body: &[Stmt]) -> (State<'a>, bool) {
        let end = body.first().map_or(start, |first| first.start().to_u32());
        let inside = self.within(start, end);
        let limit = if inside { self.at } else { u32::MAX };
        (self.apply(state, start, end, limit), inside)
    }

    /// Where the walk of `body` can start: at the last plain statement before the use
    /// that binds the name, as what the name held before it does not matter. A function
    /// that binds a name thousands of times is then not walked from its top for each
    /// use. A `return` skipped this way is not seen: a use in the dead code after it is
    /// then followed, which reports nothing a live one would not.
    fn last_assignment(&self, body: &[Stmt]) -> usize {
        let holding = body.partition_point(|stmt| stmt.end().to_u32() <= self.at);
        let Some(before) = body.get(holding).map(|stmt| stmt.start().to_u32()) else {
            return 0;
        };
        let written = self
            .bindings
            .partition_point(|binding| binding.site < before);
        let Some(last) = written.checked_sub(1).map(|last| self.bindings[last].site) else {
            return 0;
        };
        let binding = body.partition_point(|stmt| stmt.end().to_u32() <= last);
        let plain = body.get(binding).is_some_and(|stmt| {
            stmt.start().to_u32() <= last
                && !matches!(
                    stmt,
                    Stmt::If(_)
                        | Stmt::While(_)
                        | Stmt::For(_)
                        | Stmt::With(_)
                        | Stmt::Try(_)
                        | Stmt::Match(_)
                )
        });
        if plain {
            binding
        } else {
            0
        }
    }

    fn block(&self, body: &'a [Stmt], mut state: State<'a>, depth: u8) -> Out<'a> {
        if depth > MAX_DEPTH {
            return Out::Found(State::default());
        }
        let from = self.last_assignment(body);
        for (index, stmt) in body.iter().enumerate().skip(from) {
            match self.stmt(stmt, state, depth) {
                Out::Found(found) => return Out::Found(found),
                Out::Next(next) => state = next,
                Out::Stop => {
                    // What follows in the block is not reached.
                    let rest = &body[index + 1..];
                    let dead = rest.first().zip(rest.last()).is_some_and(|(first, last)| {
                        self.within(first.start().to_u32(), last.end().to_u32())
                    });
                    return if dead {
                        Out::Found(State::default())
                    } else {
                        Out::Stop
                    };
                }
            }
        }
        Out::Next(state)
    }

    fn stmt(&self, stmt: &'a Stmt, state: State<'a>, depth: u8) -> Out<'a> {
        work(1);
        let (start, end) = (stmt.start().to_u32(), stmt.end().to_u32());
        match stmt {
            Stmt::If(node) => {
                let (state, inside) = self.header(state, start, &node.body);
                if inside {
                    return Out::Found(state);
                }
                let (yes, no) = self.tested(&node.test);
                let mut outs = Vec::new();
                match self.block(&node.body, state.clone().narrow(yes), depth + 1) {
                    Out::Found(found) => return Out::Found(found),
                    out => outs.push(out),
                }
                let mut rest = state.narrow(no);
                let mut closed = false;
                for clause in &node.elif_else_clauses {
                    let branch = match &clause.test {
                        Some(test) => {
                            let (after, inside) =
                                self.header(rest, clause.start().to_u32(), &clause.body);
                            if inside {
                                return Out::Found(after);
                            }
                            let (yes, no) = self.tested(test);
                            rest = after.clone().narrow(no);
                            after.narrow(yes)
                        }
                        None => {
                            closed = true;
                            rest.clone()
                        }
                    };
                    match self.block(&clause.body, branch, depth + 1) {
                        Out::Found(found) => return Out::Found(found),
                        out => outs.push(out),
                    }
                }
                if !closed {
                    outs.push(Out::Next(rest));
                }
                merged(outs)
            }
            Stmt::While(ast::StmtWhile { body, orelse, .. })
            | Stmt::For(ast::StmtFor { body, orelse, .. }) => {
                // The iterable of a `for` is read once, before its target is bound.
                let (bound, inside) = self.header(state.clone(), start, body);
                if inside && matches!(stmt, Stmt::For(_)) {
                    return Out::Found(bound);
                }
                let body_end = body.last().map_or(end, |last| last.end().to_u32());
                let entry = self.any(bound, start, body_end);
                if inside {
                    return Out::Found(entry);
                }
                let after = match self.block(body, entry.clone(), depth + 1) {
                    Out::Found(found) => return Out::Found(found),
                    Out::Next(ran) => state.merge(&entry).merge(&ran),
                    Out::Stop => state.merge(&entry),
                };
                self.block(orelse, after, depth + 1)
            }
            Stmt::With(node) => {
                let (state, inside) = self.header(state, start, &node.body);
                if inside {
                    return Out::Found(state);
                }
                self.block(&node.body, state, depth + 1)
            }
            Stmt::Try(node) => {
                let body_end = node.body.last().map_or(start, |last| last.end().to_u32());
                // An exception can leave the body after any of its bindings.
                let raised = self.any(state.clone(), start, body_end);
                let ran = match self.block(&node.body, state, depth + 1) {
                    Out::Found(found) => return Out::Found(found),
                    out => out,
                };
                let mut outs = Vec::new();
                for handler in &node.handlers {
                    let ast::ExceptHandler::ExceptHandler(handler) = handler;
                    let (caught, inside) =
                        self.header(raised.clone(), handler.start().to_u32(), &handler.body);
                    if inside {
                        return Out::Found(caught);
                    }
                    match self.block(&handler.body, caught, depth + 1) {
                        Out::Found(found) => return Out::Found(found),
                        out => outs.push(out),
                    }
                }
                outs.push(match ran {
                    Out::Next(ran) => match self.block(&node.orelse, ran, depth + 1) {
                        Out::Found(found) => return Out::Found(found),
                        out => out,
                    },
                    out => out,
                });
                let Some(first) = node.finalbody.first() else {
                    return merged(outs);
                };
                // `finally` runs however the rest ended.
                let closing = self.any(raised, start, first.start().to_u32());
                match merged(outs) {
                    Out::Next(state) => {
                        let state = if self.within(first.start().to_u32(), end) {
                            closing
                        } else {
                            state
                        };
                        self.block(&node.finalbody, state, depth + 1)
                    }
                    _ => match self.block(&node.finalbody, closing, depth + 1) {
                        Out::Found(found) => Out::Found(found),
                        _ => Out::Stop,
                    },
                }
            }
            Stmt::Match(node) => {
                let first = node.cases.first().map_or(end, |case| case.start().to_u32());
                let inside = self.within(start, first);
                let limit = if inside { self.at } else { u32::MAX };
                let state = self.apply(state, start, first, limit);
                if inside {
                    return Out::Found(state);
                }
                let mut outs = vec![Out::Next(state.clone())];
                for case in &node.cases {
                    let (captured, inside) =
                        self.header(state.clone(), case.start().to_u32(), &case.body);
                    if inside {
                        return Out::Found(captured);
                    }
                    match self.block(&case.body, captured, depth + 1) {
                        Out::Found(found) => return Out::Found(found),
                        out => outs.push(out),
                    }
                }
                merged(outs)
            }
            _ => {
                let inside = self.within(start, end);
                let limit = if inside { self.at } else { u32::MAX };
                let state = self.apply(state, start, end, limit);
                if inside {
                    return Out::Found(state);
                }
                match stmt {
                    Stmt::Return(_) | Stmt::Raise(_) | Stmt::Continue(_) | Stmt::Break(_) => {
                        Out::Stop
                    }
                    Stmt::Assert(node) => Out::Next(state.narrow(self.tested(&node.test).0)),
                    _ => Out::Next(state),
                }
            }
        }
    }
}

/// The class `test` says `name` is an instance of when it holds, and when it does not:
/// `isinstance(name, Class)`, under `not`, and as the one such operand of an `and` (when
/// it holds) or of an `or` (when it does not). Two tests of the name in one `and` state an
/// intersection, which is not followed.
fn narrowing<'a>(test: &'a Expr, name: &str) -> (Option<&'a Expr>, Option<&'a Expr>) {
    match test {
        Expr::Call(call) => {
            let tested = matches!(call.func.as_ref(), Expr::Name(func) if func.id.as_str() == "isinstance")
                && call.arguments.keywords.is_empty();
            match &*call.arguments.args {
                [Expr::Name(subject), class] if tested && subject.id.as_str() == name => {
                    (Some(class), None)
                }
                _ => (None, None),
            }
        }
        Expr::UnaryOp(unary) if unary.op == UnaryOp::Not => {
            let (yes, no) = narrowing(&unary.operand, name);
            (no, yes)
        }
        Expr::BoolOp(both) => {
            let sides = both.values.iter().map(|value| narrowing(value, name));
            let only = |mut stated: Vec<&'a Expr>| match stated.len() {
                1 => stated.pop(),
                _ => None,
            };
            match both.op {
                BoolOp::And => (only(sides.filter_map(|(yes, _)| yes).collect()), None),
                BoolOp::Or => (None, only(sides.filter_map(|(_, no)| no).collect())),
            }
        }
        _ => (None, None),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::scopes::{Node, Scan};
    use crate::types::symbols::Source;

    /// What `name` holds at each `name.use()` of the first function of `text`: the lines
    /// of the bindings in effect, then `narrowed` when a test narrows it.
    fn held(text: &str, name: &str) -> Vec<String> {
        let source = Source::new(text, (3, 14)).expect("parses");
        let scan = Scan::new(source.body(), |_| false);
        let scope = (0..scan.scopes.len())
            .find(|&scope| matches!(scan.scopes[scope], Node::Function(_)))
            .expect("a function");
        let Node::Function(function) = scan.scopes[scope] else {
            unreachable!()
        };
        let bindings = scan.bindings(scope, name).unwrap_or(&[]);
        let needle = format!("{name}.use()");
        text.match_indices(&needle)
            .map(|(at, _)| {
                let at = u32::try_from(at).unwrap();
                let reach = reaching(&function.body, bindings, name, at).expect("in the body");
                let mut lines: Vec<String> = reach
                    .origins
                    .iter()
                    .map(|(_, site)| (text[..*site as usize].matches('\n').count() + 1).to_string())
                    .collect();
                lines.extend(reach.narrowed.map(|_| "narrowed".to_string()));
                lines.join(" ")
            })
            .collect()
    }

    #[test]
    fn a_binding_replaces_what_the_name_held() {
        let text = "def f(x):\n    x.use()\n    x = a\n    x.use()\n    x = b\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1", "3", "5"]);
    }

    #[test]
    fn branches_merge_and_a_branch_that_ends_does_not() {
        let text = "def f(x, c):\n    if c:\n        x = a\n    else:\n        x = b\n    x.use()\n    if c:\n        x = d\n        return\n    x.use()\n";
        assert_eq!(held(text, "x"), ["3 5", "3 5"]);
        let text = "def f(x, c):\n    if c:\n        x = a\n        raise E\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1"]);
    }

    #[test]
    fn a_loop_body_sees_the_bindings_of_the_loop() {
        let text = "def f(x, xs):\n    while xs:\n        x.use()\n        x = a\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 4", "1 4"]);
        let text = "def f(xs):\n    for x in xs:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["2", "2"]);
    }

    #[test]
    fn a_handler_sees_every_binding_of_its_try() {
        let text = "def f(x):\n    try:\n        x = a\n        g()\n        x = b\n    except E:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 3 5", "1 3 5"]);
    }

    #[test]
    fn isinstance_narrows_until_the_name_is_bound_again() {
        let text = "def f(x, y):\n    if isinstance(x, C):\n        x.use()\n        x = y\n        x.use()\n    else:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed", "4", "1", "1 4"]);
    }

    #[test]
    fn a_test_that_ends_the_other_branch_narrows_what_follows() {
        let text = "def f(x):\n    if not isinstance(x, C):\n        return\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed"]);
        let text = "def f(x):\n    assert isinstance(x, C)\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed"]);
        let text = "def f(x, c):\n    if c and isinstance(x, C):\n        x.use()\n    if c or isinstance(x, C):\n        x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed", "1"]);
    }

    #[test]
    fn two_tests_of_the_name_or_a_binding_in_the_test_do_not_narrow() {
        let text = "def f(x, y):\n    if isinstance(x, C) and isinstance(x, D):\n        x.use()\n    if isinstance(x, C) and (x := y):\n        x.use()\n    if not (isinstance(x, C) or isinstance(x, D)):\n        return\n    x.use()\n    if isinstance(y, C) or isinstance(x):\n        x.use()\n";
        // `:=` is taken to have run: it leaves the name unknown either way.
        assert_eq!(held(text, "x"), ["1", "4", "4", "4"]);
    }

    #[test]
    fn an_elif_is_tested_after_the_tests_before_it_failed() {
        let text = "def f(x):\n    if not isinstance(x, C):\n        x.use()\n    elif c:\n        x.use()\n    else:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1", "1 narrowed", "1 narrowed", "1"]);
        let text = "def f(x, c):\n    if c:\n        return\n    elif isinstance(x, C):\n        pass\n    else:\n        raise E\n    x.use()\n    if c:\n        pass\n    elif (x := c):\n        x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed", "11"]);
    }

    #[test]
    fn finally_runs_however_the_try_ended() {
        let text = "def f(x):\n    try:\n        x = a\n    except E:\n        x = b\n        return\n    else:\n        x = c\n    finally:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 3 5 8", "8"]);
        let text =
            "def f(x):\n    try:\n        return\n    finally:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1", ""]);
        let text = "def f(x):\n    try:\n        x.use()\n    except E as x:\n        x.use()\n";
        assert_eq!(held(text, "x"), ["1", "4"]);
    }

    #[test]
    fn a_case_binds_its_captures_and_a_with_its_target() {
        let text = "def f(x, v):\n    match x.use():\n        case [x]:\n            x.use()\n        case _ if x.use():\n            return\n    x.use()\n    with x.use() as x:\n        x.use()\n";
        assert_eq!(held(text, "x"), ["1", "3", "1", "1 3", "1 3", "8"]);
    }

    #[test]
    fn a_header_reads_the_name_before_it_binds_it() {
        let text = "def f(x, xs):\n    for x in x.use():\n        pass\n    else:\n        x.use()\n    while x.use():\n        x = a\n    if x.use():\n        pass\n    elif x.use():\n        pass\n";
        assert_eq!(held(text, "x"), ["1", "1 2", "1 2 7", "1 2 7", "1 2 7"]);
    }

    #[test]
    fn what_follows_a_statement_that_ends_the_block_is_not_reached() {
        let text =
            "def f(x):\n    while x:\n        break\n        x.use()\n    return\n    x.use()\n";
        assert_eq!(held(text, "x"), ["", ""]);
    }

    #[test]
    fn statements_nested_too_deep_are_not_followed() {
        let depth = usize::from(MAX_DEPTH) + 2;
        let mut text = "def f(x):\n".to_string();
        for level in 0..depth {
            text.push_str(&format!("{}if c:\n", "    ".repeat(level + 1)));
        }
        text.push_str(&format!("{}x.use()\n", "    ".repeat(depth + 1)));
        assert_eq!(held(&text, "x"), [""]);
    }

    #[test]
    fn many_bindings_are_walked_once() {
        let mut text = "def f(x):\n".to_string();
        for _ in 0..3000 {
            text.push_str("    x = a\n    x.use()\n");
        }
        let found = held(&text, "x");
        assert_eq!(found.len(), 3000);
        assert_eq!(found[2999], "6000");
    }

    #[test]
    fn a_name_tested_twice_is_not_followed() {
        let text = "def f(x):\n    if isinstance(x, C):\n        if isinstance(x, D):\n            x.use()\n        x.use()\n    assert isinstance(x, C)\n    if not isinstance(x, D):\n        return\n    x.use()\n    x = a\n    x.use()\n";
        // The use stands for a name that is not followed. After the inner `if`, only one
        // side tested the name twice: it is what its binding gives it.
        assert_eq!(held(text, "x"), ["4", "1", "9", "10"]);
    }

    #[test]
    fn more_bindings_than_are_followed_leave_the_name_unknown() {
        let mut text = "def f(x, c):\n".to_string();
        for _ in 0..MAX_LIVE {
            text.push_str("    if c:\n        x = a\n");
        }
        text.push_str("    x.use()\n    x = b\n    x.use()\n");
        let found = held(&text, "x");
        let unknown = (2 * MAX_LIVE + 2).to_string();
        let last = (2 * MAX_LIVE + 3).to_string();
        assert_eq!(
            found,
            [unknown, last],
            "the use itself stands for the unknown"
        );
    }

    /// The work of placing every use of `text`.
    fn cost(text: &str) -> usize {
        WORK.with(|work| work.set(0));
        let _ = held(text, "x");
        WORK.with(std::cell::Cell::get)
    }

    #[test]
    fn the_walk_of_a_use_does_not_grow_with_the_bindings_before_it() {
        // Twice the statements, each use walked: four times the work at most, whether
        // the name is bound in every statement or under a condition.
        let plain =
            |count: usize| "def f(x):\n".to_string() + &"    x = a\n    x.use()\n".repeat(count);
        let branched = |count: usize| {
            "def f(x, c):\n".to_string() + &"    if c:\n        x = a\n    x.use()\n".repeat(count)
        };
        assert!(
            cost(&plain(400)) <= 4 * 400,
            "a plain assignment restarts the walk"
        );
        assert!(cost(&plain(800)) <= 2 * cost(&plain(400)) + 16);
        let (small, large) = (cost(&branched(200)), cost(&branched(400)));
        assert!(large <= 5 * small, "{small} then {large}");
        assert!(small <= 200 * 200 * (2 + 2 * MAX_LIVE), "{small}");
    }

    #[test]
    fn a_use_outside_the_body_is_not_placed() {
        let source = Source::new("def f(x=y.use()):\n    pass\n", (3, 14)).unwrap();
        let scan = Scan::new(source.body(), |_| false);
        let Node::Function(function) = scan.scopes[1] else {
            panic!("a function")
        };
        assert!(reaching(&function.body, &[], "y", 9).is_none());
    }
}
